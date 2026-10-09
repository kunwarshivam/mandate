"""Journal spec v0.36 §9.13's `VerificationRun` version 2 reference vectors (DEC-788 item 1, DEC-789):
each range's trusted `start`, its `checked` count, and the check it could not finish
(`incomplete`), with rules 132 and 133 and the three-way `result`.

The schema and the rules live in `control.py`, beside version 1's, so one validator judges every
closed schema. This module builds the `verification_runs` section: a base draft of a passed, a
failed, and an incomplete run, an invalid draft for every new member type and every clause of rules
132 and 133, and valid drafts for the cases a rule might be misread to refuse. Version 1's own
vectors stay in `records_access`, unchanged, because the journal still registers version 1 (§8).
It checks the section with oracles of its own: each run's result is recomputed from its ranges by a
precedence table rather than rule 133's predicate, each range's count and start are recomputed from
the range's bounds, and the vocabulary and tenant checks of `audit.py` are applied again. Every
seeded bug is shown caught.
"""

from __future__ import annotations

import copy
import re

from audit import AUDITOR, CLIENT, FOREIGN_STREAM, OPERATOR_ACTOR, VOCABULARY, member_names, workspace_of
from common import apply_change, change, delete, digest_strings
from control import (
    AGENT_STREAM,
    SERVICES,
    STREAM,
    USER,
    WORKSPACE,
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

SPEC = "docs/specs/journal.md v0.36 §9.13 and §11 (DEC-788, DEC-789)"
AT = "2026-10-09T18:30:00.000000000Z"
IDS = {
    "run_passed": "01J8Z3V1A000000000000000V1",
    "run_failed": "01J8Z3V2A000000000000000V2",
    "run_incomplete": "01J8Z3V3A000000000000000V3",
}
EXPORT_ID = "01J8Z3R2A000000000000000R2"
ANCHOR_ID = "01J8Z3A1A000000000000000A1"
ACCOUNT_STREAM = f"acct:{WORKSPACE}:01J8Z2ACCT00000000000000A1"
GENESIS = "0" * 64
MANIFEST = "a" * 64
V2_VOCABULARY = VOCABULARY | {"start", "kind", "manifest_hash", "anchor_event_id", "checked", "incomplete"}

GENESIS_START = {"kind": "genesis", "manifest_hash": None, "anchor_event_id": None}
MANIFEST_START = {"kind": "manifest", "manifest_hash": MANIFEST, "anchor_event_id": None}
ANCHOR_START = {"kind": "anchor", "manifest_hash": None, "anchor_event_id": ANCHOR_ID}


def checked_range(stream, first, last, prev, head, start, checked, failure=None, incomplete=None) -> dict:
    return {
        "stream_id": stream,
        "from_seq": first,
        "to_seq": last,
        "prev_hash": prev,
        "to_hash": head,
        "start": dict(start),
        "checked": checked,
        "failure": failure,
        "incomplete": incomplete,
    }


def envelope(name: str, actor: dict, payload: dict, causation=None) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": IDS[name],
        "stream_id": STREAM,
        "event_type": "VerificationRun",
        "schema_version": 2,
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
    passed = [
        checked_range(ACCOUNT_STREAM, 1, 40, GENESIS, "1" * 64, GENESIS_START, 40),
        checked_range(AGENT_STREAM, 12, 30, "2" * 64, "3" * 64, MANIFEST_START, 19),
        checked_range(STREAM, 1, 9, GENESIS, "4" * 64, GENESIS_START, 9),
    ]
    failed = [
        checked_range(ACCOUNT_STREAM, 1, 40, GENESIS, None, GENESIS_START, 17, {"check": "rehash_mismatch", "seq": 17}),
        checked_range(STREAM, 1, 9, GENESIS, "4" * 64, GENESIS_START, 9),
    ]
    unfinished = [
        checked_range(AGENT_STREAM, 12, 30, "2" * 64, "3" * 64, ANCHOR_START, 19, None, "tsa_token_invalid"),
    ]
    return {
        "run_passed": envelope("run_passed", SERVICES, {"trigger": "weekly", "ranges": passed, "result": "pass"}),
        "run_failed": envelope(
            "run_failed", USER, {"trigger": "request", "ranges": failed, "result": "fail"}, EXPORT_ID
        ),
        "run_incomplete": envelope(
            "run_incomplete", AUDITOR, {"trigger": "request", "ranges": unfinished, "result": "incomplete"}
        ),
    }


BASES = base_drafts()
RANGE_PATH = re.compile(r"payload\.ranges\[(\d+)\]\.(.+)")


def folded(base: str, changes: list[dict]) -> list[dict]:
    """`changes` in the vectors' form, which replaces whole payload members: every change inside a
    range is folded into one replacement of the base draft's `ranges`, after the other changes."""
    ranges = copy.deepcopy(BASES[base]["payload"]["ranges"])
    out, inside = [], False
    for item in changes:
        found = RANGE_PATH.fullmatch(item["path"])
        if not found:
            out.append(item)
            continue
        apply_change(ranges[int(found.group(1))], {**item, "path": found.group(2)})
        inside = True
    return [*out, change("payload.ranges", ranges)] if inside else out


def refs_kept(base: str, changes: list[dict]) -> list[dict]:
    changes = folded(base, changes)
    draft = copy.deepcopy(BASES[base])
    for item in changes:
        apply_change(draft, item)
    return [*changes, change("artifact_refs", sorted(digest_strings(draft["payload"])))]


def invalid(name, clause, base, changes, reason, path, also=()):
    return control_invalid(name, clause, base, refs_kept(base, changes), reason, path, also)


def valid(name, clause, base, changes):
    return control_valid(name, clause, base, refs_kept(base, changes))


R0 = "payload.ranges[0]"
R1 = "payload.ranges[1]"
R2 = "payload.ranges[2]"
# Each new member, a value of the wrong JSON kind (`schema`) and, where its type constrains a string,
# a string of the wrong form (`non_canonical`).
MEMBER_CASES = {
    "run_incomplete": (
        (f"{R0}.start", "anchor", None),
        (f"{R0}.start.kind", 7, "cold_store"),
        (f"{R0}.start.anchor_event_id", 7, "anchor_one"),
        (f"{R0}.checked", "19", None),
        (f"{R0}.incomplete", 7, "seq_gap"),
        ("payload.result", True, "unproven"),
    ),
    "run_passed": ((f"{R1}.start.manifest_hash", 7, "sha256:" + MANIFEST),),
}
NULLED = {
    "run_incomplete": (f"{R0}.start", f"{R0}.start.kind", f"{R0}.checked", "payload.result"),
}


def seeded_key(path: str) -> str:
    """The `loose` and `nullable` seeded-bug key suffix of a member: its record and name."""
    depth = "start" if ".start." in path else "failure" if ".failure." in path else "range" if "]." in path else "payload"
    return f"{depth}.{path.rsplit('.', 1)[-1]}"


def member_drafts() -> list[dict]:
    out = []
    for base, cases in MEMBER_CASES.items():
        for path, wrong_kind, wrong_form in cases:
            label = path.removeprefix("payload.")
            out.append(invalid(f"{base}.{label}.kind", "§9.13 types, version 2", base, [change(path, wrong_kind)], "schema", path))
            if wrong_form is not None:
                out.append(
                    invalid(f"{base}.{label}.form", "§9.13 types, version 2", base, [change(path, wrong_form)], "non_canonical", path)
                )
    for base, members in NULLED.items():
        for path in members:
            label = path.removeprefix("payload.")
            out.append(invalid(f"{base}.{label}.null", "§9.13 types, version 2", base, [change(path, None)], "schema", path))
    for member in ("start", "checked", "incomplete"):
        out.append(
            invalid(
                f"range_without_{member}",
                "§9.13 closed: version 2's range holds every member",
                "run_incomplete",
                [delete(f"{R0}.{member}")],
                "schema",
                f"{R0}.{member}",
            )
        )
    out.append(
        invalid(
            "start_extra_member",
            "§9.13 closed: a start names its record, never a hash the caller chose",
            "run_incomplete",
            [change(f"{R0}.start.prev_hash", "2" * 64)],
            "schema",
            f"{R0}.start.prev_hash",
        )
    )
    return out


def v1_shape(ranges: list[dict]) -> list[dict]:
    return [{k: v for k, v in r.items() if k not in ("start", "checked", "incomplete")} for r in ranges]


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule, or the rules its `also` lists; together they cover every
    version-2 member type and every clause of rules 132 and 133."""
    ok, bad, unfinished = "run_passed", "run_failed", "run_incomplete"
    return [
        *member_drafts(),
        invalid(
            "version_1_records_no_incomplete",
            "§9.13 versions: version 1's result is pass or fail, so it cannot record an incomplete run",
            unfinished,
            [change("schema_version", 1), change("payload.ranges", v1_shape(BASES[unfinished]["payload"]["ranges"]))],
            "non_canonical",
            "payload.result",
        ),
        invalid(
            "version_1_carries_no_start",
            "§9.13 versions: version 1 is not edited (§8), so its range has no start, count, or incomplete check",
            unfinished,
            [change("schema_version", 1), change("payload.result", "pass")],
            "schema",
            f"{R0}.checked",
            also=[("schema", f"{R0}.incomplete"), ("schema", f"{R0}.start")],
        ),
        invalid(
            "version_3_unregistered",
            "§9.13 versions: no version 3",
            ok,
            [change("schema_version", 3)],
            "unknown_schema",
            "payload",
        ),
        invalid(
            "v2_another_workspace",
            "rule 107 judges version 2",
            ok,
            [change(f"{R0}.stream_id", FOREIGN_STREAM)],
            "schema",
            f"{R0}.stream_id",
        ),
        invalid(
            "v2_scheduled_run_by_a_user",
            "rule 110 judges version 2",
            ok,
            [change("actor", USER)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "v2_client_verifies",
            "§3 rule 83: a client never verifies, at either version",
            unfinished,
            [change("actor", CLIENT)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "v2_requested_run_by_an_operator",
            "rule 110: platform staff do not run a verification, at either version",
            unfinished,
            [change("actor", OPERATOR_ACTOR)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "v2_range_check_with_a_seq",
            "rule 111 judges version 2: `segment_gap` is reported for the range (workspace API §4.8.1, DEC-786)",
            bad,
            [change(f"{R0}.failure", {"check": "segment_gap", "seq": 17})],
            "schema",
            f"{R0}.failure.seq",
        ),
        invalid(
            "v2_passed_range_without_its_head",
            "rule 111 judges version 2: an incomplete range names the head it walked",
            unfinished,
            [change(f"{R0}.to_hash", None)],
            "schema",
            f"{R0}.to_hash",
        ),
        invalid(
            "manifest_start_without_its_hash",
            "rule 132: a manifest start names its manifest",
            ok,
            [change(f"{R1}.start.manifest_hash", None)],
            "schema",
            f"{R1}.start.manifest_hash",
        ),
        invalid(
            "genesis_start_with_a_manifest",
            "rule 132: only a manifest start names a manifest",
            ok,
            [change(f"{R0}.start.manifest_hash", MANIFEST)],
            "schema",
            f"{R0}.start.manifest_hash",
        ),
        invalid(
            "anchor_start_without_its_anchor",
            "rule 132: an anchor start names its anchor",
            unfinished,
            [change(f"{R0}.start.anchor_event_id", None)],
            "schema",
            f"{R0}.start.anchor_event_id",
        ),
        invalid(
            "manifest_start_with_an_anchor",
            "rule 132: only an anchor start names an anchor",
            ok,
            [change(f"{R1}.start.anchor_event_id", ANCHOR_ID)],
            "schema",
            f"{R1}.start.anchor_event_id",
        ),
        invalid(
            "genesis_start_after_seq_1",
            "rule 132: a genesis start enters at seq 1",
            ok,
            [change(f"{R1}.start", GENESIS_START)],
            "schema",
            f"{R1}.start.kind",
        ),
        invalid(
            "anchor_start_at_seq_1",
            "rule 132: an anchor start's leaf is at from_seq − 1, at least 1 (rule 113)",
            unfinished,
            [change(f"{R0}.from_seq", 1), change(f"{R0}.prev_hash", GENESIS), change(f"{R0}.checked", 30)],
            "schema",
            f"{R0}.start.kind",
        ),
        invalid(
            "checked_below_zero",
            "§4.4 and rule 132: a count is an integer from 0",
            bad,
            [change(f"{R0}.checked", -1)],
            "schema",
            f"{R0}.checked",
        ),
        invalid(
            "checked_past_the_range",
            "rule 132: checked is at most to_seq − from_seq + 1",
            bad,
            [change(f"{R0}.checked", 41)],
            "schema",
            f"{R0}.checked",
        ),
        invalid(
            "passed_range_short_of_its_end",
            "rule 132: a passed range walked every event (workspace API AU-8)",
            ok,
            [change(f"{R1}.checked", 18)],
            "schema",
            f"{R1}.checked",
        ),
        invalid(
            "incomplete_range_short_of_its_end",
            "rule 132: an incomplete range walked every event too",
            unfinished,
            [change(f"{R0}.checked", 18)],
            "schema",
            f"{R0}.checked",
        ),
        invalid(
            "failed_range_also_incomplete",
            "rule 132: a failed range records its failure, never an incomplete check too",
            bad,
            [change(f"{R0}.incomplete", "tsa_token_invalid")],
            "schema",
            f"{R0}.incomplete",
        ),
        invalid(
            "anchor_start_passed",
            "rule 132: an anchor start's token cannot be proven yet (DEC-789, DEC-265 item 1), so its range never passes",
            unfinished,
            [change(f"{R0}.incomplete", None), change("payload.result", "pass")],
            "schema",
            f"{R0}.incomplete",
        ),
        invalid(
            "incomplete_reported_as_pass",
            "rule 133: a run with an incomplete range and no failure is incomplete",
            unfinished,
            [change(f"{R0}.start", MANIFEST_START), change("payload.result", "pass")],
            "schema",
            "payload.result",
        ),
        invalid(
            "incomplete_reported_as_fail",
            "rule 133: an incomplete check alone never fails a run (no false SEV-1)",
            unfinished,
            [change("payload.result", "fail")],
            "schema",
            "payload.result",
        ),
        invalid(
            "fail_reported_as_incomplete",
            "rule 133: a failure outranks an incomplete check",
            bad,
            [change(f"{R1}.incomplete", "tsa_token_invalid"), change("payload.result", "incomplete")],
            "schema",
            "payload.result",
        ),
        invalid(
            "pass_reported_as_incomplete",
            "rule 133: a run with nothing unfinished is not incomplete",
            ok,
            [change("payload.result", "incomplete")],
            "schema",
            "payload.result",
        ),
        invalid(
            "fail_reported_as_pass",
            "rule 133",
            bad,
            [change("payload.result", "pass")],
            "schema",
            "payload.result",
        ),
    ]


def valid_drafts() -> list[dict]:
    """Cases a rule might be misread to refuse; each must be accepted."""
    ok, bad, unfinished = "run_passed", "run_failed", "run_incomplete"
    return [
        valid(
            "manifest_start_at_seq_1",
            "rule 132: a first segment's manifest is a start at seq 1, not only genesis (rule 116)",
            ok,
            [change(f"{R0}.start", {**MANIFEST_START, "manifest_hash": "b" * 64})],
        ),
        valid(
            "anchor_start_at_seq_2",
            "rule 132: an anchor whose leaf is seq 1",
            unfinished,
            [change(f"{R0}.from_seq", 2), change(f"{R0}.checked", 29)],
        ),
        valid(
            "anchor_start_failed",
            "rule 132: an anchor start's range may fail; a failure needs no incomplete check",
            unfinished,
            [
                change(f"{R0}.failure", {"check": "anchor_root_mismatch", "seq": None}),
                change(f"{R0}.incomplete", None),
                change("payload.result", "fail"),
            ],
        ),
        valid(
            "failed_at_its_first_event",
            "rule 132: a range may fail having walked nothing past its start",
            bad,
            [change(f"{R0}.failure", {"check": "prev_hash_mismatch", "seq": 1}), change(f"{R0}.checked", 0)],
        ),
        valid(
            "failed_after_walking_every_event",
            "rule 132: a range check may fail a range whose events were all walked",
            bad,
            [change(f"{R0}.failure", {"check": "anchor_head_mismatch", "seq": 40}), change(f"{R0}.checked", 40)],
        ),
        valid(
            "token_without_its_imprint_fails",
            "§11 Incomplete: a token without the anchor's imprint is a real `tsa_token_invalid`, reported for the range",
            unfinished,
            [
                change(f"{R0}.failure", {"check": "tsa_token_invalid", "seq": None}),
                change(f"{R0}.incomplete", None),
                change("payload.result", "fail"),
            ],
        ),
        valid(
            "in_range_anchor_leaves_a_genesis_range_incomplete",
            "§11 Incomplete (DEC-789 item 7): an in-range anchor's token check, stamped or null, cannot finish",
            ok,
            [change(f"{R2}.incomplete", "tsa_token_invalid"), change("payload.result", "incomplete")],
        ),
        valid(
            "failure_outranks_another_ranges_incomplete",
            "rule 133: one failed range fails the run whatever another range left unfinished",
            bad,
            [change(f"{R1}.incomplete", "tsa_token_invalid")],
        ),
        valid(
            "requested_run_by_a_service_account",
            "rule 110 at version 2",
            unfinished,
            [change("actor", SERVICES)],
        ),
        valid(
            "restore_drill_incomplete",
            "rules 110 and 133: a restore drill's run may end incomplete; a token-only incomplete passes the drill (DEC-789 item 9, infrastructure design OPS-8)",
            unfinished,
            [change("actor", SERVICES), change("payload.trigger", "restore_drill")],
        ),
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.tenant",
    "drafts.vocabulary",
    "drafts.outcome",
    "drafts.count",
    "drafts.start",
    "invalid_drafts",
    "valid_drafts",
)
# The run's result from the outcomes of its ranges, by precedence: a table, not rule 133's predicate.
PRECEDENCE = ("fail", "incomplete", "pass")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def range_outcome(r: dict) -> str:
    if r["failure"]:
        return "fail"
    return "incomplete" if r["incomplete"] else "pass"


def accepted(section: dict) -> list[tuple[str, dict]]:
    out = [(name, copy.deepcopy(d)) for name, d in section["drafts"].items()]
    out += [(case["name"], draft_for(section, case)) for case in section["valid_drafts"]]
    return out


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    for name, draft in accepted(section):
        home = workspace_of(draft["stream_id"])
        ranges = draft["payload"]["ranges"]
        foreign = [r["stream_id"] for r in ranges if workspace_of(r["stream_id"]) != home]
        if foreign:
            problems.append(found("drafts.tenant", f"{name} names another workspace's streams {foreign}"))
        extra = member_names(draft["payload"]) - V2_VOCABULARY
        if extra:
            problems.append(found("drafts.vocabulary", f"{name} carries {sorted(extra)}"))
        outcomes = {range_outcome(r) for r in ranges}
        want = next(o for o in PRECEDENCE if o in outcomes)
        if draft["payload"]["result"] != want:
            problems.append(found("drafts.outcome", f"{name}: ranges {sorted(outcomes)}, result {draft['payload']['result']}"))
        for r in ranges:
            length = len(range(r["from_seq"], r["to_seq"] + 1))
            if not (r["checked"] in range(length + 1) and (r["failure"] or r["checked"] == length)):
                problems.append(found("drafts.count", f"{name}: {r['stream_id']} walked {r['checked']} of {length}"))
            start = r["start"]
            named = {k for k in ("manifest_hash", "anchor_event_id") if start[k] is not None}
            expected = {"genesis": set(), "manifest": {"manifest_hash"}, "anchor": {"anchor_event_id"}}[start["kind"]]
            entry = {"genesis": r["from_seq"] == 1, "manifest": True, "anchor": r["from_seq"] > 1}[start["kind"]]
            unproven = start["kind"] == "anchor" and range_outcome(r) == "pass"
            if named != expected or not entry or unproven:
                problems.append(found("drafts.start", f"{name}: {r['stream_id']} start {start} at {r['from_seq']}"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case))
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.132.manifest",
    "rule.132.anchor",
    "rule.132.start_seq",
    "rule.132.checked",
    "rule.132.walked",
    "rule.132.one_outcome",
    "rule.132.anchor_unproven",
    "rule.133",
    "rule.133.incomplete_as_pass",
    "boundary.rule_132_anchor_seq",
    "boundary.rule_132_checked_ceiling",
    "rule.111.seq",
    "rule.111.to_hash",
    "rule.107.workspace",
    "rule.110",
    "record.extra",
    "record.missing",
    *sorted({f"loose.{seeded_key(path)}" for cases in MEMBER_CASES.values() for path, _, _ in cases}),
    *sorted({f"nullable.{seeded_key(path)}" for members in NULLED.values() for path in members}),
)


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    def base_range(s, name, i=0):
        return s["drafts"][name]["payload"]["ranges"][i]

    def foreign(s):
        base_range(s, "run_passed")["stream_id"] = FOREIGN_STREAM

    def instrument(s):
        base_range(s, "run_incomplete")["start"]["instrument"] = "AAPL"

    def incomplete_as_pass(s):
        s["drafts"]["run_incomplete"]["payload"]["result"] = "pass"

    def short_count(s):
        base_range(s, "run_passed", 1)["checked"] = 18

    def anchor_at_genesis(s):
        r = base_range(s, "run_passed")
        r["start"] = dict(ANCHOR_START)

    return [
        ("a base draft breaks rule 133", "drafts.valid", mutated(lambda s: s["drafts"]["run_failed"]["payload"].update(result="incomplete"))),
        ("a base run names another workspace's stream", "drafts.tenant", mutated(foreign)),
        ("a base start carries an instrument", "drafts.vocabulary", mutated(instrument)),
        ("an incomplete run is reported as passed", "drafts.outcome", mutated(incomplete_as_pass)),
        ("a passed range is short of its end", "drafts.count", mutated(short_count)),
        ("an anchor start at seq 1 passes", "drafts.start", mutated(anchor_at_genesis)),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "checked_past_the_range")["expect"].update(path=f"{R0}.to_seq")),
        ),
        (
            "a valid draft breaks a rule",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "anchor_start_at_seq_2")["changes"].append(change("payload.result", "pass"))),
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
            escaped.append(f"verification-run validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"verification-run check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"verification-run vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
