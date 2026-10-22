"""Journal spec v0.69 §9.31's reference vectors for `CredentialEnrolled` and `CredentialRemoved`
(DEC-2108): a member's passkeys with the workspace's relying party, which the API's passkey verifier
folds and judges an assertion against.

The schemas and rules 249 and 250 live in `control.py`, beside §9.2's other control-stream records,
so one validator judges every closed schema. This module builds the `credentials` section: a base
draft of each record, an invalid draft for every member type and each clause of the two rules, valid
drafts for the members no rule reads, and control streams for rule 251. Its own oracle reads the
drafts' members directly, without the validator, and every seeded bug is shown caught.
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

SPEC = "docs/specs/journal.md v0.69 §9.31 (DEC-2108)"
AT = "2026-10-19T15:00:05.250000000Z"
COOLED = "2026-10-20T15:00:05.250000000Z"
EVENT = "01JAA3P4A000000000000000E1"
LATER = "01JAA3P4A000000000000000E2"
THIRD = "01JAA3P4A000000000000000E3"
CHALLENGE = "01JAA3P0A000000000000000H1"
MEMBER = "01JAA2N0A000000000000000P1"
OTHER_MEMBER = "01JAA2N0A000000000000000P2"
CREDENTIAL = "AAECAwQFBgcICQoLDA0ODw"
OTHER_CREDENTIAL = "EBESExQVFhcYGRobHB0eHw"
PUBLIC_KEY = "sha256:" + "5" * 64
FIRST = "first_enrolled"
STEPPED = "stepped_up_enrolled"
REMOVED = "removed"
STEP_UP = {"assertion_id": CHALLENGE, "authenticated_at": AT, "method": "passkey"}


def envelope(event_type: str, payload: dict, event_id: str, actor: str) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id,
        "stream_id": STREAM,
        "event_type": event_type,
        "schema_version": 1,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": None,
        "correlation_id": None,
        "actor": {**USER, "id": actor},
        "config_refs": {},
        "payload": payload,
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
    }


def enrolled(event_id: str = EVENT, credential: str = CREDENTIAL, member: str = MEMBER, stepped: bool = False) -> dict:
    payload = {
        "member": member,
        "kind": "passkey",
        "credential": credential,
        "public_key": PUBLIC_KEY,
        "alg": "es256",
        "sign_count": 0,
        "cool_off_ends": None if stepped else COOLED,
        "step_up": copy.deepcopy(STEP_UP) if stepped else None,
    }
    return envelope("CredentialEnrolled", payload, event_id, member)


def removed(event_id: str = LATER, credential: str = CREDENTIAL, member: str = MEMBER) -> dict:
    payload = {"member": member, "credential": credential, "step_up": copy.deepcopy(STEP_UP)}
    return envelope("CredentialRemoved", payload, event_id, member)


def base_drafts() -> dict[str, dict]:
    return {FIRST: enrolled(), STEPPED: enrolled(stepped=True), REMOVED: removed()}


def invalid(name, clause, base, changes, reason, path, also=()):
    return control_invalid(name, clause, base, changes, reason, path, also)


def valid(name, clause, base, changes):
    return control_valid(name, clause, base, changes)


# Each member, a value of the wrong JSON kind (`schema`) and a string of the wrong form
# (`non_canonical`), on the record that holds it.
MEMBER_CASES = (
    (FIRST, "member", 7, "member-01"),
    (FIRST, "credential", 7, "AAECAwQFBgcICQoLDA0ODw=="),
    (FIRST, "public_key", 7, "5" * 64),
    (FIRST, "cool_off_ends", 7, "2026-10-20T15:00:05Z"),
    (REMOVED, "member", 7, "member-01"),
    (REMOVED, "credential", 7, "AAECAwQF+gcICQoLDA0ODw"),
)
CREDENTIAL_SHAPES = (
    ("short", "AAECAwQFBgcICQoLDA0OD"[:21]),
    ("fifteen_bytes", "A" * 20),
    ("one_over_a_block", "A" * 25),
    ("too_long", "A" * 1366),
    ("padded", "AAECAwQFBgcICQoLDA0ODw="),
)
NULLED = ((FIRST, "member"), (FIRST, "kind"), (FIRST, "credential"), (FIRST, "public_key"), (FIRST, "alg"),
          (FIRST, "sign_count"), (REMOVED, "member"), (REMOVED, "credential"), (REMOVED, "step_up"))
# A changed `ref` member also leaves `artifact_refs` naming the digest it no longer holds.
REF_MEMBERS = ("public_key",)


def also_for(member: str) -> tuple[tuple[str, str], ...]:
    return (("artifact_refs", "artifact_refs"),) if member in REF_MEMBERS else ()


def member_drafts() -> list[dict]:
    out = []
    for base, member, wrong_kind, wrong_form in MEMBER_CASES:
        path = f"payload.{member}"
        also = also_for(member)
        out.append(invalid(f"{base}.{member}.kind", "§9.31 types", base, [change(path, wrong_kind)], "schema", path, also))
        out.append(invalid(f"{base}.{member}.form", "§9.31 types", base, [change(path, wrong_form)], "non_canonical", path, also))
    for shape, value in CREDENTIAL_SHAPES:
        out.append(invalid(f"credential.{shape}", "§9.31 `credential_id`", FIRST, [change("payload.credential", value)], "non_canonical", "payload.credential"))
    # A negative integer has no canonical JSON (journal spec §4), so the journal refuses it before any
    # rule and no vector can carry one: `sign_count`'s lower bound needs no case of its own.
    for name, value in (("past_32_bits", 4294967296), ("text", "0"), ("boolean", True)):
        out.append(invalid(f"sign_count.{name}", "§9.31 `sign_count`", FIRST, [change("payload.sign_count", value)], "schema", "payload.sign_count"))
    out.append(invalid("alg.rs256", "§9.31 `passkey_alg`", FIRST, [change("payload.alg", "rs256")], "non_canonical", "payload.alg"))
    out.append(invalid("alg.kind", "§9.31 `passkey_alg`", FIRST, [change("payload.alg", 7)], "schema", "payload.alg"))
    for base, member in NULLED:
        path = f"payload.{member}"
        out.append(invalid(f"{base}.{member}.null", "§9.31 types", base, [change(path, None)], "schema", path, also_for(member)))
    out.append(invalid("missing", "§9.31 closed", FIRST, [delete("payload.alg")], "schema", "payload.alg"))
    out.append(invalid("extra", "§9.31 closed", REMOVED, [change("payload.reason", "lost")], "schema", "payload.reason"))
    return out


SYSTEM_ACTOR = {"kind": "system", "id": MEMBER, "version": "1", "build": "sha256:" + "c" * 64}


def rule_drafts() -> list[dict]:
    return [
        invalid("another_kind", "rule 249: the one kind is `passkey`", FIRST, [change("payload.kind", "recovery_code")], "schema", "payload.kind"),
        invalid("enrolled_by_a_system", "rule 249: the actor is a user", FIRST, [change("actor", SYSTEM_ACTOR)], "schema", "actor.kind"),
        invalid("enrolled_for_another", "rule 249: the actor is the member", FIRST, [change("actor.id", OTHER_MEMBER)], "schema", "actor.id"),
        invalid(
            "enrolled_with_cli_confirm",
            "rule 249: a passkey step-up",
            STEPPED,
            [change("payload.step_up.method", "cli_confirm")],
            "schema",
            "payload.step_up.method",
        ),
        invalid("first_without_cool_off", "rule 249: a first enrolment cools off", FIRST, [change("payload.cool_off_ends", None)], "schema", "payload.cool_off_ends"),
        invalid(
            "first_cools_a_second_short",
            "rule 249: exactly 86 400 s",
            FIRST,
            [change("payload.cool_off_ends", "2026-10-20T15:00:04.250000000Z")],
            "schema",
            "payload.cool_off_ends",
        ),
        invalid(
            "first_cools_from_the_whole_second",
            "rule 249: to the nanosecond",
            FIRST,
            [change("payload.cool_off_ends", "2026-10-20T15:00:05.000000000Z")],
            "schema",
            "payload.cool_off_ends",
        ),
        invalid(
            "stepped_up_cools_off",
            "rule 249: one stepped up never cools off",
            STEPPED,
            [change("payload.cool_off_ends", COOLED)],
            "schema",
            "payload.cool_off_ends",
        ),
        invalid("removed_by_a_system", "rule 250: the actor is a user", REMOVED, [change("actor", SYSTEM_ACTOR)], "schema", "actor.kind"),
        invalid("removed_for_another", "rule 250: the actor is the member", REMOVED, [change("actor.id", OTHER_MEMBER)], "schema", "actor.id"),
        invalid(
            "removed_with_cli_confirm",
            "rule 250: a passkey step-up",
            REMOVED,
            [change("payload.step_up.method", "cli_confirm")],
            "schema",
            "payload.step_up.method",
        ),
    ]


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every member type and rules 249 and 250."""
    return [
        *member_drafts(),
        *rule_drafts(),
        invalid("enrolled_at_version_2", "§9.31: closed at schema version 1 only", FIRST, [change("schema_version", 2)], "unknown_schema", "payload"),
        invalid("removed_at_version_2", "§9.31: closed at schema version 1 only", REMOVED, [change("schema_version", 2)], "unknown_schema", "payload"),
    ]


def valid_drafts() -> list[dict]:
    """The members no rule reads, and the cases a rule might be misread to refuse."""
    longest = "A" * 1364
    return [
        valid("eddsa", "§9.31 `passkey_alg`", FIRST, [change("payload.alg", "eddsa")]),
        valid("kept_counter", "§9.31 `sign_count`: the top of 32 bits", FIRST, [change("payload.sign_count", 4294967295)]),
        valid("shortest_id", "§9.31 `credential_id`: 16 bytes", FIRST, [change("payload.credential", OTHER_CREDENTIAL)]),
        valid("longest_id", "§9.31 `credential_id`: 1023 bytes", FIRST, [change("payload.credential", longest)]),
        valid("another_member_as_its_own_actor", "rules 249 and 250: a member enrols for itself",
              FIRST, [change("payload.member", OTHER_MEMBER), change("actor.id", OTHER_MEMBER)]),
        valid("in_live", "§9.31: a passkey serves `live` too (identity spec §7.3)", REMOVED, [change("environment", "live")]),
        valid("whole_second", "rule 249: a whole-second enrolment cools off from its whole second",
              FIRST, [change("event_time", "2026-10-19T15:00:05.000000000Z"),
                      change("payload.cool_off_ends", "2026-10-20T15:00:05.000000000Z")]),
    ]


# --------------------------------------------------------------------------- rule 251

def stream_failure(records: list[dict], skip: frozenset[str] = frozenset()) -> int | None:
    """The index of the first record that breaks rule 251, or None."""
    ever: set = set()
    held: dict[str, str] = {}
    for i, record in enumerate(records):
        p = record["payload"]
        if record["event_type"] == "CredentialEnrolled":
            key = (p["member"], p["credential"]) if "stream.251.per_member" in skip else p["credential"]
            if key in ever and "stream.251.twice" not in skip:
                return i
            ever.add(key)
            held[p["credential"]] = p["member"]
        elif record["event_type"] == "CredentialRemoved":
            ours = held.get(p["credential"]) == p["member"] or "stream.251.removal" in skip
            if not ours:
                return i
            held.pop(p["credential"], None)
    return None


def stream_cases() -> list[dict]:
    def case(name, clause, records, index):
        expect = {"outcome": "Valid"} if index is None else {"outcome": "Invalid", "rule": "251", "index": index}
        return {"name": name, "clause": clause, "records": records, "expect": expect}

    return [
        case(
            "enrolled_then_removed",
            "rule 251: one enrolment, one removal",
            [enrolled(), enrolled(LATER, OTHER_CREDENTIAL, stepped=True), removed(THIRD)],
            None,
        ),
        case("enrolled_twice", "rule 251: a credential is enrolled once", [enrolled(), enrolled(LATER, stepped=True)], 1),
        case(
            "enrolled_again_after_removal",
            "rule 251: never enrolled again",
            [enrolled(), removed(LATER), enrolled(THIRD, stepped=True)],
            2,
        ),
        case("removed_unenrolled", "rule 251: only an enrolled credential is removed", [removed(EVENT)], 0),
        case(
            "removed_by_another_member",
            "rule 251: only by the member who enrolled it",
            [enrolled(), removed(LATER, CREDENTIAL, OTHER_MEMBER)],
            1,
        ),
        case("removed_twice", "rule 251: not removed twice", [enrolled(), removed(LATER), removed(THIRD)], 2),
        case(
            "enrolled_by_another_member",
            "rule 251: a credential is enrolled once on the stream, whoever enrols it",
            [enrolled(), enrolled(LATER, CREDENTIAL, OTHER_MEMBER)],
            1,
        ),
        case(
            "second_unstepped_enrolment",
            "§9.31: enrolment gating is the API's; the journal admits a second enrolment with no step-up",
            [enrolled(), enrolled(LATER, OTHER_CREDENTIAL)],
            None,
        ),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = ("drafts.valid", "drafts.cool_off", "invalid_drafts", "valid_drafts", "streams.records", "streams.rule")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def oracle_index(records: list[dict]) -> int | None:
    """Rule 251 counted apart from `stream_failure`: each credential's events, in order."""
    history: dict[str, list[tuple[str, str]]] = {}
    for i, record in enumerate(records):
        p = record["payload"]
        events = history.setdefault(p["credential"], [])
        events.append((record["event_type"], p["member"]))
        kinds = [kind for kind, _ in events]
        if kinds[0] != "CredentialEnrolled" or kinds.count("CredentialEnrolled") > 1 or kinds.count("CredentialRemoved") > 1:
            return i
        if kinds[-1] == "CredentialRemoved" and events[-1][1] != events[0][1]:
            return i
    return None


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    first, stepped = section["drafts"][FIRST]["payload"], section["drafts"][STEPPED]["payload"]
    if first["step_up"] is not None or first["cool_off_ends"] != COOLED or stepped["cool_off_ends"] is not None:
        problems.append(found("drafts.cool_off", "the first enrolment does not cool off, or the stepped-up one does"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case))
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
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
    *dict.fromkeys(f"loose.payload.{member}" for _, member, _, _ in MEMBER_CASES),
    "types.credential_id_length",
    "types.sign_count_range",
    "rule.249.kind",
    "rule.249.user",
    "rule.249.member",
    "rule.249.passkey",
    "rule.249.cool_off",
    "rule.249.fraction",
    "rule.249.stepped_up_cools",
    "rule.250.user",
    "rule.250.member",
    "rule.250.passkey",
)
STREAM_MUTANTS = ("stream.251.twice", "stream.251.removal", "stream.251.per_member")


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    return [
        ("the base removal is written by another member", "drafts.valid", mutated(lambda s: s["drafts"][REMOVED]["actor"].update(id=OTHER_MEMBER))),
        (
            "the stepped-up base draft carries no step-up and no cool-off",
            "drafts.cool_off",
            mutated(lambda s: s["drafts"][FIRST]["payload"].update(cool_off_ends=None)),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "alg.rs256")["expect"].update(path="payload.kind")),
        ),
        (
            "a valid draft enrols for another member",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "eddsa")["changes"].append(change("actor.id", OTHER_MEMBER))),
        ),
        (
            "a stream record is written by a system",
            "streams.records",
            mutated(lambda s: case(s, "streams", "enrolled_then_removed")["records"][2].update(actor=SYSTEM_ACTOR)),
        ),
        (
            "a stream case expects no refusal of a second enrolment",
            "streams.rule",
            mutated(lambda s: case(s, "streams", "enrolled_twice").update(expect={"outcome": "Valid"})),
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
            escaped.append(f"credential validator mutant {mutant}")
    for mutant in STREAM_MUTANTS:
        skip = frozenset([mutant])
        if not any(stream_failure(c["records"], skip) != c["expect"].get("index") for c in section["streams"]):
            escaped.append(f"credential stream mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"credential check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"credential vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "streams": stream_cases(),
    }
