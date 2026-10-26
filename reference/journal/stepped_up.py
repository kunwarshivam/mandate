"""Journal spec v0.84's reference vectors (DEC-2131): `MandateConfirmed` version 3 (§9.9) and
`AgentDeployed` version 3 (§9.24), which carry the passkey step-up evidence §9.32's rule 253 reads.

The schemas and rules 267 and 268 live in `control.py`, beside versions 1 and 2, so one validator
judges every closed schema. This module builds the `stepped_up_records` section:

1. base drafts of both version-3 records, and of their version-2 records for comparison;
2. an invalid draft for every member type of `step_up`, for each clause of rules 267 and 268, and
   for rules 18, 22, 77, 78 and 83 at version 3, and valid drafts at both ends of the window and for
   the cases a rule might be misread to refuse;
3. batches for rule 253: a version 3 then its `StepUpAsserted` commits, and a version 2 never does;
4. §9.2's mapping of each version 3 to its version 2's fact, rule 200 included;
5. control streams for rule 201 and §9.21's manager over version-3 deployments.

Every draft, batch and stream expectation is typed from the rules; a mapping's expectation is its
version-2 twin's fact, computed by `deployed.py`'s mapping, whose own vectors pin it. The independent oracles read a version 3 as its version
2 with `step_up` dropped and judge that through `deployed.py`'s version-2 readers, whose own
vectors pin them, and every seeded bug is shown caught by the check it is registered against.
"""

from __future__ import annotations

import copy
from datetime import datetime, timedelta, timezone

import clients
import deployed
import step_up_assertions
import workspace
from common import apply_change, artifact_ref, change, delete
from control import SERVICES, USER, check_of, reported, violations

SPEC = "docs/specs/journal.md v0.84 §9.9, §9.24, §9.32 (DEC-2131)"
CHALLENGE = step_up_assertions.CHALLENGE
OTHER_CHALLENGE = step_up_assertions.OTHER_CHALLENGE
ASSERTED = "01JAA3P4A000000000000000S9"
OTHER_OWNER = "01JAA2N0A000000000000000P9"


def at(base: str, seconds: int, nanos: int = 0) -> str:
    """A §4.7 timestamp `seconds` and `nanos` before `base`, which is on a whole second."""
    day = datetime.strptime(base[:19], "%Y-%m-%dT%H:%M:%S").replace(tzinfo=timezone.utc)
    whole = day - timedelta(seconds=seconds + (1 if nanos else 0))
    fraction = (10**9 - nanos) % 10**9
    return whole.strftime("%Y-%m-%dT%H:%M:%S") + f".{fraction:09d}Z"


def after(base: str) -> str:
    """One nanosecond after `base`."""
    return base[:20] + "000000001Z"


def evidence(event_time: str, seconds: int = 10) -> dict:
    return {"assertion_id": CHALLENGE, "authenticated_at": at(event_time, seconds), "method": "passkey"}


def stepped(draft: dict) -> dict:
    """`draft`, a version 2, as its version 3: the same members, then the passkey `step_up`."""
    out = copy.deepcopy(draft)
    out["schema_version"] = 3
    out["payload"]["step_up"] = evidence(out["event_time"])
    return out


def as_version_2(draft: dict) -> dict:
    """The oracles' reading of a version 3: its version 2, `step_up` dropped (DEC-2131 item 4)."""
    out = copy.deepcopy(draft)
    if out["event_type"] in ("MandateConfirmed", "AgentDeployed") and out["schema_version"] == 3:
        out["schema_version"] = 2
        out["payload"].pop("step_up", None)
    return out


CONFIRMED_V2 = workspace.BASES["confirmed_v2"]
DEPLOYED_V2 = deployed.DRAFTS["deployed_v2"]
DRAFTS = {
    "confirmed_v3": stepped(CONFIRMED_V2),
    "deployed_v3": stepped(DEPLOYED_V2),
    "confirmed_v2": copy.deepcopy(CONFIRMED_V2),
    "deployed_v2": copy.deepcopy(DEPLOYED_V2),
}
CONFIRMED_AT = CONFIRMED_V2["event_time"]
DEPLOYED_AT = DEPLOYED_V2["event_time"]


def invalid(name, clause, base, changes, reason, path):
    expect = {"outcome": "Invalid", "reason": reason, "path": path}
    return {"name": f"{base}.{name}", "clause": clause, "base_draft": base, "changes": changes, "expect": expect}


def valid(name, clause, base, changes):
    return {"name": f"{base}.{name}", "clause": clause, "base_draft": base, "changes": changes, "expect": {"outcome": "Valid"}}


def draft_for(section: dict, case: dict) -> dict:
    return deployed.draft_for(section, case)


def step_up_cases(base: str, number: str, event_time: str) -> list[dict]:
    """The member types of `step_up` and the evidence clauses of rule `number`, on `base`."""
    s = "payload.step_up"
    clause = f"rule {number}"
    return [
        invalid("step_up.null", "§9.2's `step_up`: never null at version 3", base, [change(s, None)], "schema", s),
        invalid("step_up.missing", "§9.2: every member is present", base, [delete(s)], "schema", s),
        invalid("step_up.kind", "§9.2's `step_up` type", base, [change(s, "passkey")], "schema", s),
        invalid("assertion_id.kind", "§9.2's `step_up` type", base, [change(f"{s}.assertion_id", 7)], "schema", f"{s}.assertion_id"),
        invalid("authenticated_at.kind", "§9.2's `step_up` type", base, [change(f"{s}.authenticated_at", 7)], "schema", f"{s}.authenticated_at"),
        invalid("authenticated_at.form", "§4.7: a timestamp", base, [change(f"{s}.authenticated_at", "2026-10-08 13:59:50")], "non_canonical", f"{s}.authenticated_at"),
        invalid("method.kind", "§9.2's `step_up` type", base, [change(f"{s}.method", 7)], "schema", f"{s}.method"),
        invalid("method.missing", "§9.2's `step_up` type: closed", base, [delete(f"{s}.method")], "schema", f"{s}.method"),
        invalid("step_up.extra", "§9.2's `step_up` type: closed", base, [change(f"{s}.note", "x")], "schema", f"{s}.note"),
        invalid("cli_confirm", f"{clause}: a passkey's step-up", base, [change(f"{s}.method", "cli_confirm")], "schema", f"{s}.method"),
        invalid("oidc", f"{clause}: a passkey's step-up", base, [change(f"{s}.method", "oidc")], "schema", f"{s}.method"),
        invalid("after_the_instant", f"{clause}: authenticated no later than `event_time`", base,
                [change(f"{s}.authenticated_at", after(event_time))], "schema", f"{s}.authenticated_at"),
        invalid("stale", f"{clause}: no more than 300 seconds before, to the nanosecond", base,
                [change(f"{s}.authenticated_at", at(event_time, 300, 1))], "schema", f"{s}.authenticated_at"),
        invalid("at_version_4", "v0.84: versions 1 to 3 only", base, [change("schema_version", 4)], "unknown_schema", "payload"),
        invalid("extra", "closed", base, [change("payload.note", "x")], "schema", "payload.note"),
    ]


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover `step_up`, rules 267 and 268, and the
    version-2 rules version 3 keeps."""
    c, d = "confirmed_v3", "deployed_v3"
    other = change("payload.mandate_version", deployed.OTHER_VERSION)
    other_refs = change("artifact_refs", sorted([deployed.OTHER_VERSION, deployed.RECORD]))
    return [
        *step_up_cases(c, "267", CONFIRMED_AT),
        invalid("rule_18", "rule 18 at version 3: paths strictly ascending", c,
                [change("payload.confirmed_paths", ["/risk", "/capital"])], "non_canonical", "payload.confirmed_paths"),
        invalid("rule_77.paired", "rule 77 at version 3: `base_version` null exactly when `agent_id` is", c,
                [change("payload.agent_id", None)], "schema", "payload.base_version"),
        invalid("rule_77.moved", "rule 77 at version 3: never the version itself", c,
                [change("payload.base_version", workspace.NEW_VERSION), change("artifact_refs", sorted([workspace.NEW_VERSION, workspace.RECORD]))],
                "schema", "payload.base_version"),
        invalid("rule_78", "rule 78 at version 3: only a user confirms", c, [change("actor", dict(SERVICES))], "schema", "actor.kind"),
        invalid("by_a_client", "rule 83: a client never confirms (identity ID-11)", c, [change("actor", dict(clients.CLIENT))], "schema", "actor.kind"),
        *step_up_cases(d, "268", DEPLOYED_AT),
        invalid("rule_268.system", "rule 268: the actor is a user", d, [change("actor", dict(SERVICES))], "schema", "actor.kind"),
        invalid("by_a_client", "rule 83: a client never deploys (identity ID-11)", d, [change("actor", dict(clients.CLIENT))], "schema", "actor.kind"),
        invalid("rule_22", "rule 22 at version 3: the payload's version is the config ref's", d, [other, other_refs], "schema", "payload.mandate_version"),
        invalid("no_mandate_ref", "§9: `AgentDeployed` requires `man` at version 3 too", d, [change("config_refs", {})],
                "missing_config_ref", "config_refs.mandate_version"),
        invalid("connection_id.null", "§9.24: not nullable at version 3", d, [change("payload.connection_id", None)], "schema", "payload.connection_id"),
        invalid("connection_id.missing", "§9.24: version 2's members at version 3", d, [delete("payload.connection_id")], "schema", "payload.connection_id"),
        invalid("confirmed_v2.step_up", "§8: version 2 is unchanged, and has no `step_up`", "confirmed_v2",
                [change("payload.step_up", evidence(CONFIRMED_AT))], "schema", "payload.step_up"),
        invalid("deployed_v2.step_up", "§8: version 2 is unchanged, and has no `step_up`", "deployed_v2",
                [change("payload.step_up", evidence(DEPLOYED_AT))], "schema", "payload.step_up"),
    ]


def valid_drafts() -> list[dict]:
    """The cases a rule might be misread to refuse; each must be accepted."""
    c, d = "confirmed_v3", "deployed_v3"
    s = "payload.step_up.authenticated_at"
    return [
        valid("at_the_instant", "rule 267: 0 seconds before", c, [change(s, CONFIRMED_AT)]),
        valid("at_the_window_end", "rule 267: exactly 300 seconds before", c, [change(s, at(CONFIRMED_AT, 300))]),
        valid("new_mandate", "rule 77: no agent and no base", c, [change("payload.agent_id", None), change("payload.base_version", None),
                                                                 change("artifact_refs", sorted([workspace.NEW_VERSION, workspace.RECORD]))]),
        valid("live", "rule 267: a passkey serves `live` too (identity spec §7.3)", c, [change("environment", "live")]),
        valid("any_assertion_text", "rule 267: the assertion is rule 253's to bind", c, [change("payload.step_up.assertion_id", "assert_owner_11")]),
        valid("at_the_instant", "rule 268: 0 seconds before", d, [change(s, DEPLOYED_AT)]),
        valid("at_the_window_end", "rule 268: exactly 300 seconds before", d, [change(s, at(DEPLOYED_AT, 300))]),
        valid("another_connection", "rule 200 is the mapping's: `append` reads no mandate document", d,
              [change("payload.connection_id", deployed.OTHER_CONNECTION)]),
        valid("live", "rule 268: a passkey serves `live` too", d, [change("environment", "live")]),
        valid("another_user", "rule 268: any user; rule 253 binds the assertion's", d, [change("actor.id", OTHER_OWNER)]),
    ]


# --------------------------------------------------------------------------- rule 253


def asserted(draft: dict, challenge: str = CHALLENGE, actor: str | None = None) -> dict:
    """The `StepUpAsserted` of `draft`'s step-up (§9.32), caused by it."""
    return step_up_assertions.asserted(ASSERTED, draft["event_id"], challenge, actor=actor or draft["actor"]["id"])


def batch_cases() -> list[dict]:
    def case(name, clause, members, expect):
        return {"name": name, "clause": clause, "drafts": members, "expect": expect}

    ok = {"outcome": "Valid"}

    def at_index(index, path):
        return {"outcome": "Invalid", "index": index, "reason": "schema", "path": path}

    out = []
    for name in ("confirmed", "deployed"):
        v3, v2 = DRAFTS[f"{name}_v3"], DRAFTS[f"{name}_v2"]
        other = copy.deepcopy(v3)
        other["actor"]["id"] = OTHER_OWNER
        out += [
            case(f"{name}_v3_then_its_assertion", "rule 253: the version 3 names the challenge", [v3, asserted(v3)], ok),
            case(f"{name}_v2_then_an_assertion", "rule 253: version 2 carries no `step_up`", [v2, asserted(v2)], at_index(1, "payload.step_up")),
            case(f"{name}_v3_another_challenge", "rule 253: the challenge the record names", [v3, asserted(v3, OTHER_CHALLENGE)],
                 at_index(1, "payload.challenge")),
            case(f"{name}_v3_by_another_user", "rule 253: the same actor", [other, asserted(other, actor=USER["id"])], at_index(1, "actor.id")),
            case(f"{name}_v3_alone", "rule 253 judges only an assertion: the record's converse is the writer's", [v3], ok),
        ]
    return out


# --------------------------------------------------------------------------- §9.2's mapping


def map_confirmed(draft: dict) -> dict:
    p = draft["payload"]
    return {"refused": None, "fact": {"kind": "MandateConfirmed", "version": p["mandate_version"], "confirmed_paths": p["confirmed_paths"]}}


def mapped(draft: dict, documents: dict[str, dict], skip: frozenset[str] = frozenset()) -> dict:
    """§9.2's mapping of either record, any version."""
    if draft["event_type"] == "MandateConfirmed":
        return map_confirmed(draft)
    return deployed.map_deployed(draft, documents, skip)


def mapping_cases() -> list[dict]:
    both = ["mandate_document", "deployment_record"]

    def case(name, clause, base, changes, documents):
        draft = copy.deepcopy(DRAFTS[base])
        for item in changes:
            apply_change(draft, item)
        expect = mapped(as_version_2(draft), deployed.stored(documents))
        return {"name": name, "clause": clause, "base_draft": base, "changes": changes, "stored": documents, "expect": expect}

    return [
        case("confirmed_v3", "§9.9: version 3 maps as version 2, from the same members", "confirmed_v3", [], []),
        case("deployed_v3", "§9.24: version 3 maps as version 2, from the same document", "deployed_v3", [], both),
        case("deployed_v3_another_connection", "rule 200 at version 3: refused where the document is read", "deployed_v3",
             [change("payload.connection_id", deployed.OTHER_CONNECTION)], both),
        case("deployed_v3_not_stored", "§9.2: a document that is not stored is refused, at every version", "deployed_v3", [], []),
    ]


# --------------------------------------------------------------------------- rule 201 and §9.21


def deployed_v3(event_id: str, agent: str = deployed.AGENT, connection: str = deployed.CONNECTION) -> dict:
    return stepped(deployed.deployed(event_id, agent, connection))


def stream_cases() -> list[dict]:
    def ids(n: int, count: int) -> list[str]:
        return [f"01J9DC{n:02d}{i:02d}".ljust(26, "0") for i in range(count)]

    def case(n, name, clause, build, expect, runs):
        records = build(iter(ids(n, 8)))
        return {"name": name, "clause": clause, "records": records, "expect": expect, "processes": runs}

    ok = {"outcome": "Valid"}

    def refused(index):
        return {"outcome": "Invalid", "index": index, "rule": 201, "path": "payload.connection_id"}

    one = {"runtimes": [deployed.AGENT], "executors": [deployed.ACCOUNT]}
    e, r, s = deployed.established, deployed.revoked, deployed.stopped
    return [
        case(1, "established_then_deployed_v3", "rule 201 at version 3; §9.21: the account is the establishment's",
             lambda i: [e(next(i)), deployed_v3(next(i))], ok, one),
        case(2, "v2_and_v3_one_account", "§9.21: one executor per account, whichever version",
             lambda i: [e(next(i)), deployed.deployed(next(i)), deployed_v3(next(i), deployed.OTHER_AGENT)], ok,
             {"runtimes": [deployed.AGENT, deployed.OTHER_AGENT], "executors": [deployed.ACCOUNT]}),
        case(3, "never_established", "rule 201 at version 3: an establishment earlier on the stream",
             lambda i: [deployed_v3(next(i))], refused(0), deployed.NOTHING),
        case(4, "revoked_before", "rule 201 at version 3: not since revoked",
             lambda i: [e(next(i)), r(next(i)), deployed_v3(next(i))], refused(2), deployed.NOTHING),
        case(5, "version_1_establishment", "rule 201 at version 3: established at version 2",
             lambda i: [e(next(i), account=None), deployed_v3(next(i))], refused(1), deployed.NOTHING),
        case(6, "stopped", "§9.21: an `AgentStopped` stops a version-3 deployment",
             lambda i: [e(next(i)), deployed_v3(next(i)), s(next(i))], ok, deployed.NOTHING),
        case(7, "revoked_after", "rule 201 judges the deployment only",
             lambda i: [e(next(i)), deployed_v3(next(i)), r(next(i))], ok, one),
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = (
    "drafts.valid",
    "invalid_drafts",
    "valid_drafts",
    "batches.drafts",
    "batches.rule",
    "mappings.rule",
    "mappings.oracle",
    "streams.records",
    "streams.rule",
    "streams.processes",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def batch_expected(case: dict) -> dict | None:
    want = case["expect"]
    return None if want["outcome"] == "Valid" else {k: want[k] for k in ("index", "reason", "path")}


def stream_expected(case: dict) -> dict | None:
    want = case["expect"]
    return None if want["outcome"] == "Valid" else {k: want[k] for k in ("index", "rule", "path")}


def check_section(section: dict, skip: frozenset[str] = frozenset()) -> list[str]:
    """Every failure, so seeded bugs can be shown caught. `skip` seeds bugs in the readers."""
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
        for i, member in enumerate(case["drafts"]):
            if violations(member):
                problems.append(found("batches.drafts", f"{case['name']} draft {i}: {violations(member)}"))
        got = step_up_assertions.batch_failure(case["drafts"])
        if got != batch_expected(case):
            problems.append(found("batches.rule", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["mappings"]:
        draft = draft_for(section, case)
        documents = deployed.stored(case["stored"])
        got = mapped(draft, documents, skip)
        if got != case["expect"]:
            problems.append(found("mappings.rule", f"{case['name']}: expected {case['expect']}, got {got}"))
        twin = draft_for(section, {**case, "base_draft": case["base_draft"].replace("_v3", "_v2")})
        if mapped(twin, documents) != case["expect"]:
            problems.append(found("mappings.oracle", f"{case['name']}: the version-2 base with its changes maps otherwise"))
    for case in section["streams"]:
        records = case["records"]
        for i, record in enumerate(records):
            got = violations(record)
            if got:
                problems.append(found("streams.records", f"{case['name']} record {i}: {got}"))
        read = [as_version_2(record) for record in records]
        want = stream_expected(case)
        if deployed.stream_failure(records, skip) != want or deployed.stream_failure(read) != want:
            problems.append(found("streams.rule", f"{case['name']}: expected {case['expect']}"))
        if deployed.processes(records, skip) != case["processes"] or deployed.processes(read) != case["processes"]:
            problems.append(found("streams.processes", f"{case['name']}: expected {case['processes']}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "versions.v3",
    "rule.267.method",
    "rule.267.fresh",
    "loose.267.cli_confirm",
    "boundary.rule_267_after",
    "boundary.rule_267_window",
    "rule.268.user",
    "loose.268.system",
    "rule.268.method",
    "rule.268.fresh",
    "loose.268.cli_confirm",
    "boundary.rule_268_after",
    "boundary.rule_268_window",
    "rule.77.v2_only",
)
READER_MUTANTS = ("v3.read_as_v1",)


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    def stale_base(s):
        s["drafts"]["deployed_v3"]["payload"]["step_up"]["authenticated_at"] = at(DEPLOYED_AT, 301)

    return [
        ("a base draft's step-up is stale", "drafts.valid", mutated(stale_base)),
        ("an invalid draft's expectation differs", "invalid_drafts",
         mutated(lambda s: case(s, "invalid_drafts", "confirmed_v3.cli_confirm")["expect"].update(path="payload.step_up"))),
        ("a valid draft is stale", "valid_drafts",
         mutated(lambda s: case(s, "valid_drafts", "deployed_v3.at_the_window_end")["changes"].append(
             change("payload.step_up.authenticated_at", at(DEPLOYED_AT, 301))))),
        ("a batch's expectation differs", "batches.rule",
         mutated(lambda s: case(s, "batches", "confirmed_v2_then_an_assertion").update(expect={"outcome": "Valid"}))),
        ("a batch member is written by a system", "batches.drafts",
         mutated(lambda s: case(s, "batches", "deployed_v3_alone")["drafts"][0].update(actor=dict(SERVICES)))),
        ("a mapping's expectation and the reader disagree", "mappings.rule",
         mutated(lambda s: case(s, "mappings", "deployed_v3")["expect"].update(refused="payload.step_up"))),
        ("a mapping's version 2 maps otherwise", "mappings.oracle",
         mutated(lambda s: s["drafts"]["confirmed_v2"]["payload"].update(confirmed_paths=["/capital", "/universe"]))),
        ("a stream record is written by a system", "streams.records",
         mutated(lambda s: case(s, "streams", "stopped")["records"][1].update(actor=dict(SERVICES)))),
        ("a stream case expects no refusal", "streams.rule",
         mutated(lambda s: case(s, "streams", "never_established").update(expect={"outcome": "Valid"}))),
        ("a stream case expects nothing started", "streams.processes",
         mutated(lambda s: case(s, "streams", "established_then_deployed_v3").update(processes=deployed.NOTHING))),
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
            escaped.append(f"stepped-up validator mutant {mutant}")
    for mutant in READER_MUTANTS:
        if not check_section(section, frozenset([mutant])):
            escaped.append(f"stepped-up reader mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"stepped-up check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"stepped-up vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": copy.deepcopy(DRAFTS),
        "stream_id": deployed.STREAM,
        "artifacts": [{"name": name, "ref": artifact_ref(obj), "object": copy.deepcopy(obj)} for name, obj in deployed.ARTIFACTS.items()],
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "batches": batch_cases(),
        "mappings": mapping_cases(),
        "streams": stream_cases(),
    }
