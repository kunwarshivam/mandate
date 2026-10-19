"""Journal spec v0.71 §9.32's reference vectors for `StepUpAsserted` (DEC-2108 item 3): the assertion
behind one passkey step-up, appended immediately after the event it authorized in one batch.

The schema and rule 252 live in `control.py`, beside §9.2's other control-stream records. Rule 253
is a batch rule and stream rule 254 needs the stream, so both live here, as rule 248 does in
`owner_confirmations.py`. This module builds the `step_up_assertions` section: a base draft, an
invalid draft for every member type and each clause of rule 252, batches for each clause of rule 253,
and control streams for rule 254. Its own oracles walk the records directly, without the validator,
and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy

import credentials
import membership
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

SPEC = "docs/specs/journal.md v0.71 §9.32 (DEC-2108 item 3)"
AT = credentials.AT
BASE = "step_up_asserted"
CAUSE = credentials.LATER
ASSERTED = "01JAA3P4A000000000000000S1"
LATER = "01JAA3P4A000000000000000S2"
THIRD = "01JAA3P4A000000000000000S3"
CHALLENGE = credentials.CHALLENGE
OTHER_CHALLENGE = "01JAA3P0A000000000000000H2"
MEMBER = credentials.MEMBER
OTHER_MEMBER = credentials.OTHER_MEMBER
CREDENTIAL = credentials.CREDENTIAL
OTHER_CREDENTIAL = credentials.OTHER_CREDENTIAL
ASSERTION = "sha256:" + "6" * 64
RULE_253 = "rule 253"


def asserted(
    event_id: str = ASSERTED,
    cause: str | None = CAUSE,
    challenge: str = CHALLENGE,
    credential: str = CREDENTIAL,
    sign_count: int = 7,
    actor: str = MEMBER,
) -> dict:
    payload = {"challenge": challenge, "credential": credential, "sign_count": sign_count, "assertion": ASSERTION}
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id,
        "stream_id": STREAM,
        "event_type": "StepUpAsserted",
        "schema_version": 1,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": cause,
        "correlation_id": None,
        "actor": {**USER, "id": actor},
        "config_refs": {},
        "payload": payload,
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    return {BASE: asserted()}


def invalid(name, clause, changes, reason, path, also=()):
    return control_invalid(name, clause, BASE, changes, reason, path, also)


def valid(name, clause, changes):
    return control_valid(name, clause, BASE, changes)


MEMBER_CASES = (
    ("challenge", 7, "not-a-ulid"),
    ("credential", 7, "AAECAwQFBgcICQoLDA0ODw=="),
    ("assertion", 7, "6" * 64),
)
NULLED = ("challenge", "credential", "sign_count", "assertion")
SYSTEM_ACTOR = credentials.SYSTEM_ACTOR


def also_for(member: str) -> tuple[tuple[str, str], ...]:
    return (("artifact_refs", "artifact_refs"),) if member == "assertion" else ()


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every member type and rule 252."""
    out = []
    for member, wrong_kind, wrong_form in MEMBER_CASES:
        path = f"payload.{member}"
        out.append(invalid(f"{member}.kind", "§9.32 types", [change(path, wrong_kind)], "schema", path, also_for(member)))
        out.append(invalid(f"{member}.form", "§9.32 types", [change(path, wrong_form)], "non_canonical", path, also_for(member)))
    # A negative integer has no canonical JSON (journal spec §4): the journal refuses it at parse, so no
    # vector carries one.
    for name, value in (("past_32_bits", 4294967296), ("text", "7")):
        out.append(invalid(f"sign_count.{name}", "§9.31 `sign_count`", [change("payload.sign_count", value)], "schema", "payload.sign_count"))
    for member in NULLED:
        path = f"payload.{member}"
        out.append(invalid(f"{member}.null", "§9.32 types", [change(path, None)], "schema", path, also_for(member)))
    out.append(invalid("missing", "§9.32 closed", [delete("payload.assertion")], "schema", "payload.assertion", also_for("assertion")))
    out.append(invalid("extra", "§9.32 closed", [change("payload.method", "passkey")], "schema", "payload.method"))
    out.append(invalid("uncaused", "rule 252: names the event it authorized", [change("causation_id", None)], "schema", "causation_id"))
    out.append(invalid("by_a_system", "rule 252: the actor is a user", [change("actor", SYSTEM_ACTOR)], "schema", "actor.kind"))
    out.append(invalid("at_version_2", "§9.32: closed at schema version 1 only", [change("schema_version", 2)], "unknown_schema", "payload"))
    return out


def valid_drafts() -> list[dict]:
    """The members no rule reads."""
    return [
        valid("uncounted", "§9.31 `sign_count`: 0, a counter not kept", [change("payload.sign_count", 0)]),
        valid("in_live", "§9.32: a passkey serves `live` too (identity spec §7.3)", [change("environment", "live")]),
        valid("another_credential", "§9.32: any credential id", [change("payload.credential", OTHER_CREDENTIAL)]),
    ]


# --------------------------------------------------------------------------- rule 253

def removal(event_id: str = CAUSE, actor: str = MEMBER) -> dict:
    """The authorized event: a member removing a credential under passkey step-up (§9.31)."""
    return credentials.removed(event_id, OTHER_CREDENTIAL, actor)


def non_passkey_grant() -> dict:
    """A control-stream event whose step-up is not a passkey's, valid by its own rules: §9.12's
    `MemberInvited`, its evidence's method changed."""
    draft = copy.deepcopy(next(d for d in membership.base_drafts().values() if d["payload"].get("step_up")))
    draft["payload"]["step_up"]["method"] = "cli_confirm"
    draft["event_id"] = CAUSE
    return draft


def batch_failure(drafts: list[dict], skip: frozenset[str] = frozenset()) -> dict | None:
    """Each draft's own rules in draft order, then rule 253 on each assertion in batch order."""
    for i, draft in enumerate(drafts):
        got = violations(draft)
        if got:
            return {"index": i, "reason": got[0].reason, "path": got[0].path}
    for i, draft in enumerate(drafts):
        if draft["event_type"] != "StepUpAsserted":
            continue
        if "batch.253.adjacent" in skip:
            before = next((d for d in drafts[:i] if d["event_id"] == draft["causation_id"]), None)
        else:
            before = drafts[i - 1] if i > 0 else None
        named = before is not None and (before["event_id"] == draft["causation_id"] or "batch.253.cause" in skip)
        if not named:
            if "batch.253.present" in skip:
                continue
            return {"index": i, "reason": "schema", "path": "event_type"}
        if before["actor"]["id"] != draft["actor"]["id"] and "batch.253.actor" not in skip:
            return {"index": i, "reason": "schema", "path": "actor.id"}
        step_up = before["payload"].get("step_up")
        passkey = isinstance(step_up, dict) and (step_up.get("method") == "passkey" or "batch.253.passkey" in skip)
        if not passkey:
            return {"index": i, "reason": "schema", "path": "payload.step_up"}
        if step_up.get("assertion_id") != draft["payload"]["challenge"] and "batch.253.challenge" not in skip:
            return {"index": i, "reason": "schema", "path": "payload.challenge"}
    return None


def batch_cases() -> list[dict]:
    def case(name, clause, members, expect):
        return {"name": name, "clause": clause, "drafts": members, "expect": expect}

    ok = {"outcome": "Valid"}

    def at(index, path):
        return {"outcome": "Invalid", "index": index, "reason": "schema", "path": path}

    stepped = credentials.enrolled(THIRD, OTHER_CREDENTIAL, stepped=True)
    first = credentials.enrolled(THIRD, OTHER_CREDENTIAL)
    return [
        case("the_event_then_its_assertion", f"{RULE_253}: one batch, the event first", [removal(), asserted()], ok),
        case(
            "two_events_each_with_its_assertion",
            f"{RULE_253}: each immediately after its own",
            [removal(), asserted(), stepped, asserted(LATER, THIRD)],
            ok,
        ),
        case("alone", f"{RULE_253}: the event is in the batch", [asserted()], at(0, "event_type")),
        case("before_its_event", f"{RULE_253}: the event comes first", [asserted(), removal()], at(0, "event_type")),
        case(
            "after_another_event",
            f"{RULE_253}: immediately after the event it names",
            [removal(), stepped, asserted()],
            at(2, "event_type"),
        ),
        case("naming_another_event", f"{RULE_253}: the event its cause names", [removal(), asserted(cause=THIRD)], at(1, "event_type")),
        case("by_another_member", f"{RULE_253}: the same actor", [removal(), asserted(actor=OTHER_MEMBER)], at(1, "actor.id")),
        case("of_an_event_without_step_up", f"{RULE_253}: the event carries step-up", [first, asserted(cause=THIRD)], at(1, "payload.step_up")),
        case(
            "of_an_event_without_a_passkey",
            f"{RULE_253}: its step-up is a passkey's",
            [non_passkey_grant(), asserted(actor=non_passkey_grant()["actor"]["id"])],
            at(1, "payload.step_up"),
        ),
        case("of_another_challenge", f"{RULE_253}: the challenge the event names", [removal(), asserted(challenge=OTHER_CHALLENGE)], at(1, "payload.challenge")),
        case("a_bad_assertion", "each draft's own rules first", [removal(), asserted(cause=None)], at(1, "causation_id")),
    ]


# --------------------------------------------------------------------------- rule 254

def stream_failure(records: list[dict], skip: frozenset[str] = frozenset()) -> int | None:
    """The index of the first `StepUpAsserted` that breaks rule 254, or None."""
    last: dict[str, int] = {}
    owner: dict[str, str] = {}
    challenges: set[str] = set()
    for i, record in enumerate(records):
        p = record["payload"]
        if record["event_type"] == "CredentialEnrolled":
            last[p["credential"]] = p["sign_count"]
            owner[p["credential"]] = p["member"]
        elif record["event_type"] == "CredentialRemoved":
            last.pop(p["credential"], None)
            owner.pop(p["credential"], None)
        elif record["event_type"] == "StepUpAsserted":
            mine = owner.get(p["credential"]) == record["actor"]["id"] or "stream.254.owner" in skip
            if not mine:
                return i
            before = last.get(p["credential"], 0)
            kept = p["sign_count"] > before or (p["sign_count"] == 0 and before == 0)
            if not kept and "stream.254.counter" not in skip:
                return i
            if p["challenge"] in challenges and "stream.254.challenge" not in skip:
                return i
            challenges.add(p["challenge"])
            last[p["credential"]] = max(before, p["sign_count"])
    return None


def stream_cases() -> list[dict]:
    def case(name, clause, records, index):
        expect = {"outcome": "Valid"} if index is None else {"outcome": "Invalid", "rule": "254", "index": index}
        return {"name": name, "clause": clause, "records": records, "expect": expect}

    def enrolled(count: int) -> dict:
        record = credentials.enrolled()
        record["payload"]["sign_count"] = count
        return record

    return [
        case("rising", "rule 254: above the last", [enrolled(5), asserted(sign_count=6), asserted(LATER, challenge=OTHER_CHALLENGE, sign_count=9)], None),
        case("not_kept", "rule 254: 0 on both sides", [enrolled(0), asserted(sign_count=0), asserted(LATER, challenge=OTHER_CHALLENGE, sign_count=0)], None),
        case("equal_to_enrolment", "rule 254: not equal", [enrolled(5), asserted(sign_count=5)], 1),
        case("below_the_last", "rule 254: never backwards", [enrolled(5), asserted(sign_count=9), asserted(LATER, challenge=OTHER_CHALLENGE, sign_count=8)], 2),
        case("zero_after_kept", "rule 254: 0 only when never kept", [enrolled(5), asserted(sign_count=0)], 1),
        case("never_enrolled", "rule 254: an enrolled credential", [asserted()], 0),
        case("another_members", "rule 254: the actor's own", [enrolled(5), asserted(sign_count=6, actor=OTHER_MEMBER)], 1),
        case("after_removal", "rule 254: not removed", [enrolled(5), credentials.removed(), asserted(sign_count=6)], 2),
        case("challenge_twice", "rule 254: one assertion per challenge", [enrolled(5), asserted(sign_count=6), asserted(LATER, sign_count=9)], 2),
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = ("drafts.valid", "invalid_drafts", "valid_drafts", "batches.rule", "batches.oracle", "streams.records", "streams.rule")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def oracle_253(drafts: list[dict]) -> tuple[int, str] | None:
    """Rule 253 as a walk over the batch, keeping the last record seen."""
    previous = None
    for i, draft in enumerate(drafts):
        if draft["event_type"] == "StepUpAsserted":
            if previous is None or previous["event_id"] != draft["causation_id"]:
                return (i, "event_type")
            if previous["actor"]["id"] != draft["actor"]["id"]:
                return (i, "actor.id")
            evidence = previous["payload"].get("step_up") or {}
            if evidence.get("method") != "passkey":
                return (i, "payload.step_up")
            if evidence.get("assertion_id") != draft["payload"]["challenge"]:
                return (i, "payload.challenge")
        previous = draft
    return None


def oracle_254(records: list[dict]) -> int | None:
    """Rule 254 from each credential's own history, rebuilt at every assertion."""
    for i, record in enumerate(records):
        if record["event_type"] != "StepUpAsserted":
            continue
        credential = record["payload"]["credential"]
        history = [r for r in records[:i] if r["payload"].get("credential") == credential]
        enrolments = [r for r in history if r["event_type"] == "CredentialEnrolled"]
        removed = any(r["event_type"] == "CredentialRemoved" for r in history)
        if not enrolments or removed or enrolments[-1]["payload"]["member"] != record["actor"]["id"]:
            return i
        counts = [r["payload"]["sign_count"] for r in history if r["event_type"] in ("CredentialEnrolled", "StepUpAsserted")]
        top, now = max(counts), record["payload"]["sign_count"]
        if not (now > top or now == top == 0):
            return i
        if any(r["event_type"] == "StepUpAsserted" and r["payload"]["challenge"] == record["payload"]["challenge"] for r in records[:i]):
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
        if case["clause"].startswith(RULE_253):
            seen = oracle_253(case["drafts"])
            if seen != (None if expected is None else (expected["index"], expected["path"])):
                problems.append(found("batches.oracle", f"{case['name']}: the oracle disagrees on rule 253"))
    for case in section["streams"]:
        for i, record in enumerate(case["records"]):
            got = violations(record)
            if got:
                problems.append(found("streams.records", f"{case['name']} record {i}: {got}"))
        want = case["expect"].get("index")
        if stream_failure(case["records"]) != want or oracle_254(case["records"]) != want:
            problems.append(found("streams.rule", f"{case['name']}: expected {case['expect']}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "record.extra",
    "record.missing",
    *(f"loose.payload.{member}" for member, _, _ in MEMBER_CASES),
    "types.sign_count_range",
    "rule.252.cause",
    "rule.252.user",
)
BATCH_MUTANTS = (
    "batch.253.present",
    "batch.253.adjacent",
    "batch.253.cause",
    "batch.253.actor",
    "batch.253.passkey",
    "batch.253.challenge",
)
STREAM_MUTANTS = ("stream.254.owner", "stream.254.counter", "stream.254.challenge")


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    return [
        ("the base draft has no cause", "drafts.valid", mutated(lambda s: s["drafts"][BASE].update(causation_id=None))),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "uncaused")["expect"].update(path="actor.kind")),
        ),
        (
            "a valid draft is written by a system",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "uncounted")["changes"].append(change("actor", SYSTEM_ACTOR))),
        ),
        (
            "a batch's expectation differs",
            "batches.rule",
            mutated(lambda s: case(s, "batches", "alone")["expect"].update(index=1)),
        ),
        (
            "a rule-253 batch's draft rules and its walk disagree",
            "batches.oracle",
            mutated(lambda s: case(s, "batches", "a_bad_assertion").update(clause=f"{RULE_253}: relabelled")),
        ),
        (
            "a stream record is written by a system",
            "streams.records",
            mutated(lambda s: case(s, "streams", "rising")["records"][1].update(actor=SYSTEM_ACTOR)),
        ),
        (
            "a stream case expects no refusal of a counter that went backwards",
            "streams.rule",
            mutated(lambda s: case(s, "streams", "below_the_last").update(expect={"outcome": "Valid"})),
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
            escaped.append(f"assertion validator mutant {mutant}")
    for mutant in BATCH_MUTANTS:
        skip = frozenset([mutant])
        caught = False
        for case in section["batches"]:
            want = case["expect"]
            expected = None if want["outcome"] == "Valid" else {k: want[k] for k in ("index", "reason", "path")}
            caught |= batch_failure(case["drafts"], skip) != expected
        if not caught:
            escaped.append(f"assertion batch mutant {mutant}")
    for mutant in STREAM_MUTANTS:
        skip = frozenset([mutant])
        if not any(stream_failure(c["records"], skip) != c["expect"].get("index") for c in section["streams"]):
            escaped.append(f"assertion stream mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"assertion check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"assertion vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
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
