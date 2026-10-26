"""Journal spec v0.92 §9.6's reference vectors for `ApprovalRequested` version 3 (DEC-2515): the
runtime's approval request names, inside its hashed content, the checks its dry run deferred to
the executor's binding gate, with rule 291 on the request and rule 292 on the batch that holds it
and the `DecisionMade` that asks.

The request's schema and rule 291 live in `control.py`, beside versions 1 and 2, so one validator
judges every closed `ApprovalRequested`. The decisions are `generate.py`'s: its caller hands this
module two whole `DecisionMade` drafts and its own validator for them, so the batch is judged as
`append` judges it, each draft on its own first. This module builds the `approval_deferrals`
section: the version 3 base, an invalid draft for each clause of the schema and rule 291, the
valid drafts a rule might be misread to refuse, and the batches rule 292 judges. Its own oracle
recomputes the base's content hash with `json` and `hashlib`, and every seeded bug is shown
caught.
"""

from __future__ import annotations

import copy
from collections.abc import Callable

from common import DEFERRED_PARTS, Violation, apply_change, change, delete, digest_strings
from control import check_of, draft_for, reported, violations
from requests import PLAN, V1, V2, base_drafts, oracle_hash, restated

SPEC = "docs/specs/journal.md v0.92 §9.6 rules 291 and 292 (DEC-2515)"
V3 = "requested_v3"
DEFERRED = "payload.content.deferred_to_binding"
NAMED = ["halt", "liquidity_floor", "participation"]
"""What the deferring decision names: one unseen part, and both liquidity checks, read without
figures."""

Validator = Callable[[dict, frozenset[str]], list[Violation]]

APPROVER_SENTENCE = "These checks have not been run yet. They run when the order is placed, and may still refuse it:"
"""Mandate spec v0.12 §6.4: what the approver is shown, exactly, before each deferred check's name."""


def v3_base() -> dict:
    """Version 2's base, then the deferrals at the end of the content, under its restated hash."""
    out = copy.deepcopy(base_drafts()[V2])
    out["schema_version"] = 3
    out["payload"]["content"]["deferred_to_binding"] = list(NAMED)
    out["payload"]["content_hash"] = oracle_hash(out["payload"]["content"])
    out["artifact_refs"] = sorted(digest_strings(out["payload"]))
    return out


def bases() -> dict[str, dict]:
    return {V1: copy.deepcopy(base_drafts()[V1]), V2: copy.deepcopy(base_drafts()[V2]), V3: v3_base()}


def again(base: str, changes: list[dict]) -> list[dict]:
    """`changes` on this section's `base`, then the hash and references the changed draft states."""
    after = copy.deepcopy(bases()[base])
    for item in changes:
        apply_change(after, item)
    hashed = oracle_hash(after["payload"]["content"])
    after["payload"]["content_hash"] = hashed
    return [
        *changes,
        change("payload.content_hash", hashed),
        change("artifact_refs", sorted(digest_strings(after["payload"]))),
    ]


def invalid(name, clause, base, changes, reason, path):
    return {
        "name": name,
        "clause": clause,
        "base_draft": base,
        "changes": changes,
        "expect": {"outcome": "Invalid", "reason": reason, "path": path},
    }


def valid(name, clause, base, changes):
    return {"name": name, "clause": clause, "base_draft": base, "changes": changes, "expect": {"outcome": "Valid"}}


def named(parts: list[str]) -> list[dict]:
    return again(V3, [change(DEFERRED, list(parts))])


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule: the version split, the names' type, and rule 291."""
    return [
        invalid("v3_without_its_deferrals", "§9.6 v3: the deferrals are required", V3, again(V3, [delete(DEFERRED)]), "schema", DEFERRED),
        invalid("v3_with_null_deferrals", "§9.6 v3: the deferrals are not nullable", V3, again(V3, [change(DEFERRED, None)]), "schema", DEFERRED),
        invalid(
            "v2_with_deferrals",
            "§9.6 v2 closed: version 2 names no deferral",
            V2,
            restated(V2, [change(DEFERRED, list(NAMED))]),
            "schema",
            DEFERRED,
        ),
        invalid(
            "v3_without_a_plan",
            "§9.6 v3: the plan stays required",
            V3,
            again(V3, [delete("payload.content.execution")]),
            "schema",
            "payload.content.execution",
        ),
        invalid("deferral_unknown", "§9.6 v3: the names are §9.1's", V3, named(["exposure"]), "non_canonical", f"{DEFERRED}[0]"),
        invalid(
            "deferrals_out_of_order",
            "rule 291: ascending by bytes",
            V3,
            named(["participation", "liquidity_floor", "halt"]),
            "non_canonical",
            DEFERRED,
        ),
        invalid(
            "deferral_repeated",
            "rule 291: strictly ascending",
            V3,
            named(["halt", "halt", "liquidity_floor", "participation"]),
            "non_canonical",
            DEFERRED,
        ),
        invalid("deferrals_empty", "rule 291: never empty", V3, named([]), "schema", DEFERRED),
        invalid(
            "floor_without_participation",
            "rule 291: the liquidity checks together",
            V3,
            named(["halt", "liquidity_floor"]),
            "schema",
            DEFERRED,
        ),
        invalid(
            "participation_without_the_floor",
            "rule 291: the liquidity checks together",
            V3,
            named(["participation", "quote_ask"]),
            "schema",
            DEFERRED,
        ),
    ]


def valid_drafts() -> list[dict]:
    """Cases a rule might be misread to refuse; each must be accepted."""
    return [
        valid("one_deferral", "rule 291: one name is enough", V3, named(["eligibility"])),
        valid("every_deferral", "rule 291: all fourteen, in byte order", V3, named(list(DEFERRED_PARTS))),
        valid("figures_read", "rule 291: neither liquidity check, once figures were read", V3, named(["agent_mode", "halt"])),
    ]


def batch(name, clause, decision, request_base, changes, expect=None, request_first=False):
    """A batch of the `decision` this section holds, then a request on `request_base` that names it,
    with `changes`, or the request first when `request_first`. `expect` is `None` for a batch
    `append` accepts."""
    return {
        "name": name,
        "clause": clause,
        "decision": decision,
        "request": {"base_draft": request_base, "changes": changes},
        "order": "request_first" if request_first else "decision_first",
        "expect": expect or {"outcome": "Valid"},
    }


def batch_drafts(section: dict, case: dict) -> list[dict]:
    """The case's two drafts, in its order."""
    decision = section["decisions"][case["decision"]]
    pair = [decision, request_of(section, case, decision)]
    return pair[::-1] if case["order"] == "request_first" else pair


def refused(index: int, reason: str = "schema", path: str = DEFERRED) -> dict:
    return {"outcome": "Invalid", "index": index, "reason": reason, "path": path}


def batches() -> list[dict]:
    """Rule 292: the request names exactly what its decision deferred, and only that."""
    return [
        batch("deferring_decision_and_its_request", "rule 292", "deferring", V3, []),
        batch("plain_decision_and_a_v2_request", "rule 292: neither names a deferral", "plain", V2, []),
        batch(
            "v2_request_hides_a_deferral",
            "rule 292: absent is empty, which the decision's names are not",
            "deferring",
            V2,
            [],
            refused(1),
        ),
        batch(
            "v3_request_names_another_deferral",
            "rule 292",
            "deferring",
            V3,
            named(["agent_mode", "liquidity_floor", "participation"]),
            refused(1),
        ),
        batch(
            "v3_request_on_a_plain_decision",
            "rule 292: the decision deferred nothing",
            "plain",
            V3,
            [],
            refused(1),
        ),
        batch(
            "v1_request_hides_a_deferral",
            "rule 292 at every version",
            "deferring",
            V1,
            [],
            refused(1),
        ),
        batch(
            "request_written_before_its_decision",
            "rule 292 judges the batch, whatever the order",
            "deferring",
            V2,
            [],
            refused(0),
            request_first=True,
        ),
        batch(
            "request_naming_another_decision",
            "rule 292 binds only the decision the request names",
            "plain",
            V3,
            [change("causation_id", "01J8Z3N9A000000000000000Q0")],
        ),
    ]


def request_of(section: dict, case: dict, decision: dict) -> dict:
    """The case's request, naming `decision` unless its changes name another."""
    out = copy.deepcopy(section["drafts"][case["request"]["base_draft"]])
    out["causation_id"] = decision["event_id"]
    for item in case["request"]["changes"]:
        apply_change(out, item)
    return out


def deferrals(payload: dict) -> list[str]:
    return list(payload.get("deferred_to_binding") or [])


def batch_violations(
    drafts: list[dict], decide: Validator, skip: frozenset[str] = frozenset()
) -> list[tuple[int, Violation]]:
    """An `append` batch (§5.1), refused whole on any violation: each draft's own, in draft order,
    then rule 292 on an `ApprovalRequested` whose `DecisionMade` is in the batch."""
    out = []
    for i, draft in enumerate(drafts):
        judge = decide if draft["event_type"] == "DecisionMade" else violations
        out += [(i, v) for v in judge(draft, skip)]
    if out or "rule.292" in skip:
        return out
    decisions = {d["event_id"]: d for d in drafts if d["event_type"] == "DecisionMade"}
    for i, draft in enumerate(drafts):
        cause = decisions.get(draft["causation_id"]) if draft["event_type"] == "ApprovalRequested" else None
        if cause is None:
            continue
        if deferrals(draft["payload"]["content"]) != deferrals(cause["payload"]):
            out.append((i, Violation("rule.292", "schema", DEFERRED)))
    return out


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.hash",
    "decisions.valid",
    "decisions.named",
    "invalid_drafts",
    "valid_drafts",
    "batches",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def outcome(got: list[tuple[int, Violation]]) -> dict:
    if not got:
        return {"outcome": "Valid"}
    index, first = got[0]
    return {"outcome": "Invalid", "index": index, "reason": first.reason, "path": first.path}


def expected_batch(section: dict, case: dict) -> dict:
    """The oracle's own reading of rule 292: the sets each side names, by membership, not order,
    since each side's order is already its own rule."""
    decision = section["decisions"][case["decision"]]
    request = request_of(section, case, decision)
    if request["causation_id"] != decision["event_id"]:
        return {"outcome": "Valid"}
    theirs = set(decision["payload"].get("deferred_to_binding") or ())
    mine = set(request["payload"]["content"].get("deferred_to_binding") or ())
    return {"outcome": "Valid"} if theirs == mine else refused(0 if case["order"] == "request_first" else 1)


def check_section(section: dict, decide: Validator) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, base in section["drafts"].items():
        if violations(base):
            problems.append(found("drafts.valid", f"base draft {name}: {violations(base)}"))
        if base["payload"]["content_hash"] != oracle_hash(base["payload"]["content"]):
            problems.append(found("drafts.hash", f"base draft {name} states another hash than its content's"))
    v3 = section["drafts"][V3]
    if v3["schema_version"] != 3 or v3["payload"]["content"].get("execution") != PLAN:
        problems.append(found("drafts.valid", "the version 3 base is not version 2's plan with its deferrals"))
    for name, decision in section["decisions"].items():
        got = decide(decision, frozenset())
        if got:
            problems.append(found("decisions.valid", f"decision {name}: {got}"))
    if section["approver_sentence"] != APPROVER_SENTENCE:
        problems.append(found("decisions.named", "the approver's sentence is not mandate spec §6.4's"))
    if deferrals(section["decisions"]["deferring"]["payload"]) != NAMED:
        problems.append(found("decisions.named", "the deferring decision names other checks"))
    if "deferred_to_binding" in section["decisions"]["plain"]["payload"]:
        problems.append(found("decisions.named", "the plain decision names deferrals"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case))
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    for case in section["batches"]:
        got = outcome(batch_violations(batch_drafts(section, case), decide))
        want = case["expect"]
        if got != want or want != expected_batch(section, case):
            problems.append(found("batches", f"{case['name']}: stated {want}, got {got}, oracle {expected_batch(section, case)}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = ("rule.291.ascending", "rule.291.nonempty", "rule.291.pair", "rule.292", "record.extra", "record.missing")


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    def unhashed(s):
        s["drafts"][V3]["payload"]["content"]["deferred_to_binding"] = ["eligibility"]

    def rehashed_v2(s):
        s["drafts"][V3]["schema_version"] = 2

    return [
        ("the version 3 base changes its deferrals under the old hash", "drafts.hash", mutated(unhashed)),
        ("the version 3 base is written as version 2", "drafts.valid", mutated(rehashed_v2)),
        (
            "the deferring decision is written at version 1",
            "decisions.valid",
            mutated(lambda s: s["decisions"]["deferring"].update(schema_version=1)),
        ),
        (
            "the deferring decision names another unseen part",
            "decisions.named",
            mutated(lambda s: s["decisions"]["deferring"]["payload"].update(deferred_to_binding=["agent_mode", "liquidity_floor", "participation"])),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "deferrals_empty")["expect"].update(reason="non_canonical")),
        ),
        (
            "a valid draft changes its deferrals under the old hash",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "one_deferral")["changes"].pop()),
        ),
        (
            "a refused batch is stated accepted",
            "batches",
            mutated(lambda s: case(s, "batches", "v2_request_hides_a_deferral").update(expect={"outcome": "Valid"})),
        ),
    ]


def run_mutants(section: dict, decide: Validator) -> list[str]:
    """Every seeded bug caught; a vector mutant only by the check it is registered against."""
    escaped = []
    singles = [(copy.deepcopy(d), None) for d in section["drafts"].values()]
    singles += [(draft_for(section, c), None) for c in section["valid_drafts"]]
    singles += [(draft_for(section, c), c["expect"]) for c in section["invalid_drafts"]]
    for mutant in VALIDATOR_MUTANTS:
        skip = frozenset([mutant])
        caught = False
        for d, want in singles:
            got = violations(d, skip)
            caught |= bool(got) if want is None else not reported(got, want)
        for c in section["batches"]:
            got = outcome(batch_violations(batch_drafts(section, c), decide, skip))
            caught |= got != c["expect"]
        if not caught:
            escaped.append(f"approval deferral validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"approval deferral check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated, decide)}
        if check not in caught_by:
            escaped.append(f"approval deferral vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section(decisions: dict[str, dict]) -> dict:
    """`decisions` holds generate.py's `deferring` (version 3, naming `NAMED`, asked) and `plain`
    (version 1, asked) decisions, whole drafts."""
    return {
        "spec": SPEC,
        "approver_sentence": APPROVER_SENTENCE,
        "decisions": copy.deepcopy(decisions),
        "drafts": bases(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "batches": batches(),
    }
