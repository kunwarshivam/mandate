"""Journal spec v0.113 §9.38's reference vectors for `StepUpCoolOffWaived` (DEC-2146): a passkey step-up a
`dev` or `ci` test cell counted inside its credential's enrolment cool-off, appended immediately after
that step-up's `StepUpAsserted` in one batch.

The schema and rule 316 live in `control.py`, beside §9.32's `StepUpAsserted`. Rule 317 is a batch rule
and stream rule 318 needs the stream, so both live here, as rules 253 and 254 live in
`step_up_assertions.py`. This module builds the `cool_off_waivers` section: a base draft, an invalid
draft for every member type and each clause of rule 316, batches for each clause of rule 317, and
control streams for each clause of rule 318 and its converse. Its own oracles walk the records
directly, without the validator, and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy

import credentials
import step_up_assertions
from common import change, delete
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

SPEC = "docs/specs/journal.md v0.113 §9.38 (DEC-2146)"
BASE = "cool_off_waived"
CAUSE = step_up_assertions.CAUSE
ASSERTED = step_up_assertions.ASSERTED
WAIVED = "01JAA3P4A000000000000000W1"
LATER_ASSERTED = "01JAA3P4A000000000000000S4"
LATER_WAIVED = "01JAA3P4A000000000000000W2"
CHALLENGE = step_up_assertions.CHALLENGE
OTHER_CHALLENGE = step_up_assertions.OTHER_CHALLENGE
MEMBER = credentials.MEMBER
OTHER_MEMBER = credentials.OTHER_MEMBER
CREDENTIAL = credentials.CREDENTIAL
OTHER_CREDENTIAL = credentials.OTHER_CREDENTIAL
SYSTEM_ACTOR = credentials.SYSTEM_ACTOR
# The enrolment of `credentials.enrolled()` is at `credentials.AT` and cools off until `COOLED`, a day
# later (rule 249). A step-up ten minutes after enrolment is inside the cool-off; one at `COOLED` is not.
ENROLLED_AT = credentials.AT
COOLED = credentials.COOLED
INSIDE = "2026-10-19T15:10:05.250000000Z"
LATER_INSIDE = "2026-10-19T15:20:05.250000000Z"
AFTER = "2026-10-20T15:00:05.250000001Z"
RULE_317 = "rule 317"
RULE_318 = "rule 318"


def waived(
    event_id: str = WAIVED,
    cause: str | None = CAUSE,
    challenge: str = CHALLENGE,
    credential: str = CREDENTIAL,
    ends: str = COOLED,
    tier: str = "ci",
    actor: str = MEMBER,
    at: str = INSIDE,
) -> dict:
    payload = {"challenge": challenge, "credential": credential, "cool_off_ends": ends, "tier": tier}
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id,
        "stream_id": STREAM,
        "event_type": "StepUpCoolOffWaived",
        "schema_version": 1,
        "event_time": at,
        "clock_source": "local",
        "causation_id": cause,
        "correlation_id": None,
        "actor": {**USER, "id": actor},
        "config_refs": {},
        "payload": payload,
        "artifact_refs": [],
        "pii_refs": [],
    }


def asserted(
    event_id: str = ASSERTED,
    cause: str = CAUSE,
    challenge: str = CHALLENGE,
    credential: str = CREDENTIAL,
    sign_count: int = 1,
    actor: str = MEMBER,
    at: str = INSIDE,
) -> dict:
    """§9.32's assertion of the step-up, at `at`."""
    record = step_up_assertions.asserted(event_id, cause, challenge, credential, sign_count, actor)
    record["event_time"] = at
    return record


def base_drafts() -> dict[str, dict]:
    return {BASE: waived()}


def invalid(name, clause, changes, reason, path, also=()):
    return control_invalid(name, clause, BASE, changes, reason, path, also)


def valid(name, clause, changes):
    return control_valid(name, clause, BASE, changes)


MEMBER_CASES = (
    ("challenge", 7, "not-a-ulid"),
    ("credential", 7, "AAECAwQFBgcICQoLDA0ODw=="),
    ("cool_off_ends", 7, "2026-10-20T15:00:05Z"),
)
NULLED = ("challenge", "credential", "cool_off_ends", "tier")


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every member type and rule 316."""
    out = []
    for member, wrong_kind, wrong_form in MEMBER_CASES:
        path = f"payload.{member}"
        out.append(invalid(f"{member}.kind", "§9.38 types", [change(path, wrong_kind)], "schema", path))
        out.append(invalid(f"{member}.form", "§9.38 types", [change(path, wrong_form)], "non_canonical", path))
    for name, value in (("production", "production"), ("capitalised", "CI"), ("staging_capitalised", "Staging")):
        out.append(invalid(f"tier.{name}", "§9.38 `tier`: `dev`, `ci` or `staging` only (DEC-2150)", [change("payload.tier", value)], "non_canonical", "payload.tier"))
    out.append(invalid("tier.kind", "§9.38 `tier`: text", [change("payload.tier", 7)], "schema", "payload.tier"))
    for member in NULLED:
        path = f"payload.{member}"
        out.append(invalid(f"{member}.null", "§9.38 types", [change(path, None)], "schema", path))
    out.append(invalid("missing", "§9.38 closed", [delete("payload.tier")], "schema", "payload.tier"))
    out.append(invalid("extra", "§9.38 closed", [change("payload.reason", "test_cell")], "schema", "payload.reason"))
    out.append(invalid("uncaused", "rule 316: names the event the step-up authorized", [change("causation_id", None)], "schema", "causation_id"))
    out.append(invalid("by_a_system", "rule 316: the actor is a user", [change("actor", SYSTEM_ACTOR)], "schema", "actor.kind"))
    out.append(invalid("in_live", "rule 316: paper only", [change("environment", "live")], "schema", "environment"))
    out.append(invalid("at_version_2", "§9.38: closed at schema version 1 only", [change("schema_version", 2)], "unknown_schema", "payload"))
    return out


def valid_drafts() -> list[dict]:
    """The members no rule reads."""
    return [
        valid("in_a_dev_cell", "§9.38 `tier`: `dev`", [change("payload.tier", "dev")]),
        valid("in_a_staging_cell", "§9.38 `tier`: `staging`, before its expiry (DEC-2150)", [change("payload.tier", "staging")]),
        valid("another_credential", "§9.38: any credential id", [change("payload.credential", OTHER_CREDENTIAL)]),
    ]


# --------------------------------------------------------------------------- rule 317

def removal(event_id: str = CAUSE, actor: str = MEMBER) -> dict:
    """The authorized event: a member removing a credential under passkey step-up (§9.31)."""
    return step_up_assertions.removal(event_id, actor)


def batch_failure(drafts: list[dict], skip: frozenset[str] = frozenset()) -> dict | None:
    """Each draft's own rules and rule 253 (`step_up_assertions.batch_failure`), then rule 317 on each
    waiver in batch order."""
    earlier = step_up_assertions.batch_failure(drafts)
    if earlier is not None:
        return earlier
    for i, draft in enumerate(drafts):
        if draft["event_type"] != "StepUpCoolOffWaived":
            continue
        before = drafts[i - 1] if i > 0 else None
        if "batch.317.adjacent" in skip:
            before = next((d for d in reversed(drafts[:i]) if d["event_type"] == "StepUpAsserted"), before)
        present = before is not None and (before["event_type"] == "StepUpAsserted" or "batch.317.present" in skip)
        if not present:
            return {"index": i, "reason": "schema", "path": "event_type"}
        if before["causation_id"] != draft["causation_id"] and "batch.317.cause" not in skip:
            return {"index": i, "reason": "schema", "path": "causation_id"}
        if before["actor"]["id"] != draft["actor"]["id"] and "batch.317.actor" not in skip:
            return {"index": i, "reason": "schema", "path": "actor.id"}
        if before["payload"].get("challenge") != draft["payload"]["challenge"] and "batch.317.challenge" not in skip:
            return {"index": i, "reason": "schema", "path": "payload.challenge"}
        if before["payload"].get("credential") != draft["payload"]["credential"] and "batch.317.credential" not in skip:
            return {"index": i, "reason": "schema", "path": "payload.credential"}
        if before["event_time"] != draft["event_time"] and "batch.317.time" not in skip:
            return {"index": i, "reason": "schema", "path": "event_time"}
    return None


def batch_cases() -> list[dict]:
    def case(name, clause, members, expect):
        return {"name": name, "clause": clause, "drafts": members, "expect": expect}

    ok = {"outcome": "Valid"}

    def at(index, path):
        return {"outcome": "Invalid", "index": index, "reason": "schema", "path": path}

    return [
        case("the_event_its_assertion_then_its_waiver", f"{RULE_317}: one batch, in order", [removal(), asserted(), waived()], ok),
        case("the_event_and_its_assertion_alone", f"{RULE_317}: a waiver only where one was needed", [removal(), asserted()], ok),
        case("alone", f"{RULE_317}: its assertion is in the batch", [waived()], at(0, "event_type")),
        case("after_the_event", f"{RULE_317}: immediately after the assertion", [removal(), waived()], at(1, "event_type")),
        case(
            "twice",
            f"{RULE_317}: immediately after the assertion, so once",
            [removal(), asserted(), waived(), waived(LATER_WAIVED)],
            at(3, "event_type"),
        ),
        case("of_another_cause", f"{RULE_317}: the same cause", [removal(), asserted(), waived(cause=LATER_WAIVED)], at(2, "causation_id")),
        case("by_another_member", f"{RULE_317}: the same actor", [removal(), asserted(), waived(actor=OTHER_MEMBER)], at(2, "actor.id")),
        case("of_another_challenge", f"{RULE_317}: the same challenge", [removal(), asserted(), waived(challenge=OTHER_CHALLENGE)], at(2, "payload.challenge")),
        case(
            "of_another_credential",
            f"{RULE_317}: the same credential",
            [removal(), asserted(), waived(credential=OTHER_CREDENTIAL)],
            at(2, "payload.credential"),
        ),
        case(
            "at_another_instant",
            f"{RULE_317}: the same event_time",
            [removal(), asserted(), waived(at=LATER_INSIDE)],
            at(2, "event_time"),
        ),
        case("a_bad_waiver", "each draft's own rules first", [removal(), asserted(), waived(cause=None)], at(2, "causation_id")),
        case("a_bad_assertion", "rule 253 before rule 317", [asserted(), waived()], at(0, "event_type")),
    ]


# --------------------------------------------------------------------------- rule 318

def stream_failure(records: list[dict], skip: frozenset[str] = frozenset()) -> int | None:
    """The index of the first record that breaks rule 318, or None: a waiver that breaks a clause, or an
    assertion inside its credential's cool-off that the next record does not waive."""
    owner: dict[str, str] = {}
    ends: dict[str, str | None] = {}
    waived_challenges: set[str] = set()
    owed: tuple[int, str] | None = None
    for i, record in enumerate(records):
        p = record["payload"]
        event_type = record["event_type"]
        if owed is not None:
            settled = event_type == "StepUpCoolOffWaived" and p["challenge"] == owed[1]
            if not settled and "stream.318.converse" not in skip:
                return owed[0]
            owed = None
        if event_type == "CredentialEnrolled":
            owner[p["credential"]] = p["member"]
            ends[p["credential"]] = p["cool_off_ends"]
        elif event_type == "CredentialRemoved":
            owner.pop(p["credential"], None)
            ends.pop(p["credential"], None)
        elif event_type == "StepUpAsserted":
            cooling = ends.get(p["credential"])
            if cooling is not None and cooling > record["event_time"]:
                owed = (i, p["challenge"])
        elif event_type == "StepUpCoolOffWaived":
            if owner.get(p["credential"]) != record["actor"]["id"] and "stream.318.owner" not in skip:
                return i
            if ends.get(p["credential"]) != p["cool_off_ends"] and "stream.318.ends" not in skip:
                return i
            if not p["cool_off_ends"] > record["event_time"] and "stream.318.after" not in skip:
                return i
            if p["challenge"] in waived_challenges and "stream.318.challenge" not in skip:
                return i
            waived_challenges.add(p["challenge"])
    if owed is not None and "stream.318.converse" not in skip:
        return owed[0]
    return None


def stream_cases() -> list[dict]:
    def case(name, clause, records, index):
        expect = {"outcome": "Valid"} if index is None else {"outcome": "Invalid", "rule": "318", "index": index}
        return {"name": name, "clause": clause, "records": records, "expect": expect}

    first = credentials.enrolled()
    second = credentials.enrolled(credentials.THIRD, OTHER_CREDENTIAL, stepped=True)
    later = asserted(LATER_ASSERTED, challenge=OTHER_CHALLENGE, sign_count=2, at=LATER_INSIDE)
    later_waiver = waived(LATER_WAIVED, challenge=OTHER_CHALLENGE, at=LATER_INSIDE)
    return [
        case("waived_inside_the_cool_off", f"{RULE_318}: the waiver of an assertion inside the cool-off",
             [first, asserted(), waived()], None),
        case("two_step_ups_each_waived", f"{RULE_318}: each assertion inside the cool-off waived",
             [first, asserted(), waived(), later, later_waiver], None),
        case("in_a_dev_cell", f"{RULE_318}: either test tier", [first, asserted(), waived(tier="dev")], None),
        case("at_the_cool_offs_end", f"{RULE_318}: a step-up at `cool_off_ends` needs no waiver",
             [first, asserted(at=COOLED)], None),
        case("after_the_cool_off", f"{RULE_318}: after it, none", [first, asserted(at=AFTER)], None),
        case("a_second_enrolment", f"{RULE_318}: a credential with no cool-off needs no waiver",
             [second, asserted(credential=OTHER_CREDENTIAL)], None),
        case("unwaived_inside_the_cool_off", f"{RULE_318}: the converse, an assertion inside the cool-off waived",
             [first, asserted()], 1),
        case("unwaived_then_another_record", f"{RULE_318}: the converse, immediately after",
             [first, asserted(), later, waived()], 1),
        case("the_second_unwaived", f"{RULE_318}: the converse, for each assertion",
             [first, asserted(), waived(), later], 3),
        case("waived_at_the_cool_offs_end", f"{RULE_318}: `cool_off_ends` after the waiver's time",
             [first, asserted(at=COOLED), waived(at=COOLED)], 2),
        case("waived_after_the_cool_off", f"{RULE_318}: `cool_off_ends` after the waiver's time",
             [first, asserted(at=AFTER), waived(at=AFTER)], 2),
        case("another_end", f"{RULE_318}: the enrolment's own `cool_off_ends`",
             [first, asserted(), waived(ends=AFTER)], 2),
        case("a_second_enrolment_waived", f"{RULE_318}: the enrolment's own `cool_off_ends`, `null`",
             [second, asserted(credential=OTHER_CREDENTIAL), waived(credential=OTHER_CREDENTIAL)], 2),
        case("another_members", f"{RULE_318}: the actor's own credential",
             [first, asserted(), waived(actor=OTHER_MEMBER)], 2),
        case("never_enrolled", f"{RULE_318}: an enrolled credential", [asserted(), waived()], 1),
        case("after_removal", f"{RULE_318}: not removed", [first, credentials.removed(), waived()], 2),
        case("waived_twice", f"{RULE_318}: one waiver per challenge",
             [first, asserted(), waived(), waived(LATER_WAIVED)], 3),
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = ("drafts.valid", "invalid_drafts", "valid_drafts", "batches.rule", "batches.oracle", "streams.records", "streams.rule")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def oracle_317(drafts: list[dict]) -> tuple[int, str] | None:
    """Rule 317 as a walk over the batch, keeping the last record seen."""
    previous = None
    for i, draft in enumerate(drafts):
        if draft["event_type"] == "StepUpCoolOffWaived":
            if previous is None or previous["event_type"] != "StepUpAsserted":
                return (i, "event_type")
            for path, mine, theirs in (
                ("causation_id", draft["causation_id"], previous["causation_id"]),
                ("actor.id", draft["actor"]["id"], previous["actor"]["id"]),
                ("payload.challenge", draft["payload"]["challenge"], previous["payload"]["challenge"]),
                ("payload.credential", draft["payload"]["credential"], previous["payload"]["credential"]),
                ("event_time", draft["event_time"], previous["event_time"]),
            ):
                if mine != theirs:
                    return (i, path)
        previous = draft
    return None


def enrolment(history: list[dict], credential: str) -> dict | None:
    """The credential's enrolment, while it is not removed, from its own history."""
    mine = [r for r in history if r["payload"].get("credential") == credential and r["event_type"] in ("CredentialEnrolled", "CredentialRemoved")]
    return mine[-1] if mine and mine[-1]["event_type"] == "CredentialEnrolled" else None


def oracle_318(records: list[dict]) -> int | None:
    """Rule 318 from each record's own history, rebuilt at every assertion and waiver."""
    for i, record in enumerate(records):
        p = record["payload"]
        if record["event_type"] == "StepUpAsserted":
            enrolled = enrolment(records[:i], p["credential"])
            cooling = enrolled["payload"]["cool_off_ends"] if enrolled else None
            if cooling is not None and cooling > record["event_time"]:
                following = records[i + 1] if i + 1 < len(records) else None
                if not (following and following["event_type"] == "StepUpCoolOffWaived" and following["payload"]["challenge"] == p["challenge"]):
                    return i
        if record["event_type"] == "StepUpCoolOffWaived":
            enrolled = enrolment(records[:i], p["credential"])
            if enrolled is None or enrolled["payload"]["member"] != record["actor"]["id"]:
                return i
            if enrolled["payload"]["cool_off_ends"] != p["cool_off_ends"] or not p["cool_off_ends"] > record["event_time"]:
                return i
            if any(r["event_type"] == "StepUpCoolOffWaived" and r["payload"]["challenge"] == p["challenge"] for r in records[:i]):
                return i
    return None


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
    for case in section["batches"]:
        want = case["expect"]
        got = batch_failure(case["drafts"])
        expected = None if want["outcome"] == "Valid" else {k: want[k] for k in ("index", "reason", "path")}
        if got != expected:
            problems.append(found("batches.rule", f"{case['name']}: expected {want}, got {got}"))
        if case["clause"].startswith(RULE_317):
            seen = oracle_317(case["drafts"])
            if seen != (None if expected is None else (expected["index"], expected["path"])):
                problems.append(found("batches.oracle", f"{case['name']}: the oracle disagrees on rule 317"))
    for case in section["streams"]:
        for i, record in enumerate(case["records"]):
            got = violations(record)
            if got:
                problems.append(found("streams.records", f"{case['name']} record {i}: {got}"))
        want = case["expect"].get("index")
        if stream_failure(case["records"]) != want or oracle_318(case["records"]) != want:
            problems.append(found("streams.rule", f"{case['name']}: expected {case['expect']}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "record.extra",
    "record.missing",
    *(f"loose.payload.{member}" for member, _, _ in MEMBER_CASES),
    "loose.payload.tier",
    "rule.316.cause",
    "rule.316.user",
    "rule.316.paper",
)
BATCH_MUTANTS = (
    "batch.317.present",
    "batch.317.adjacent",
    "batch.317.cause",
    "batch.317.actor",
    "batch.317.challenge",
    "batch.317.credential",
    "batch.317.time",
)
STREAM_MUTANTS = ("stream.318.owner", "stream.318.ends", "stream.318.after", "stream.318.challenge", "stream.318.converse")


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    return [
        ("the base draft is in live", "drafts.valid", mutated(lambda s: s["drafts"][BASE].update(environment="live"))),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "in_live")["expect"].update(path="actor.kind")),
        ),
        (
            "a valid draft names a production tier",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "in_a_dev_cell")["changes"].append(change("payload.tier", "production"))),
        ),
        (
            "a batch's expectation differs",
            "batches.rule",
            mutated(lambda s: case(s, "batches", "alone")["expect"].update(index=1)),
        ),
        (
            "a rule-317 batch's draft rules and its walk disagree",
            "batches.oracle",
            mutated(lambda s: case(s, "batches", "a_bad_assertion").update(clause=f"{RULE_317}: relabelled")),
        ),
        (
            "a stream record is written by a system",
            "streams.records",
            mutated(lambda s: case(s, "streams", "waived_inside_the_cool_off")["records"][2].update(actor=SYSTEM_ACTOR)),
        ),
        (
            "a stream case expects no refusal of an unwaived step-up inside the cool-off",
            "streams.rule",
            mutated(lambda s: case(s, "streams", "unwaived_inside_the_cool_off").update(expect={"outcome": "Valid"})),
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
            escaped.append(f"waiver validator mutant {mutant}")
    for mutant in BATCH_MUTANTS:
        skip = frozenset([mutant])
        caught = False
        for case in section["batches"]:
            want = case["expect"]
            expected = None if want["outcome"] == "Valid" else {k: want[k] for k in ("index", "reason", "path")}
            caught |= batch_failure(case["drafts"], skip) != expected
        if not caught:
            escaped.append(f"waiver batch mutant {mutant}")
    for mutant in STREAM_MUTANTS:
        skip = frozenset([mutant])
        if not any(stream_failure(c["records"], skip) != c["expect"].get("index") for c in section["streams"]):
            escaped.append(f"waiver stream mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"waiver check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"waiver vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "batches": batch_cases(),
        "streams": stream_cases(),
    }
