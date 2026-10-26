"""Journal spec v0.42 §9.6's reference vectors for `ApprovalRequested` (DEC-460, DEC-942): the
runtime's approval request at version 1, and at version 2, which binds the order builder's
execution plan as one more content member under the content hash.

The schemas and rules live in `control.py`, beside §9.6's and §9.7's other records, so one
validator judges every closed schema. This module builds the `approval_requests` section: a base
draft at each version, an invalid draft for every plan member and rule, and valid drafts for the
cases a rule might be misread to refuse. A draft whose change alters the content restates its hash
and references, so it breaks only the rule it names. Its own oracle recomputes each base draft's
content hash with `json` and `hashlib`, without the validator's canonicalizer, and every seeded bug
is shown caught.
"""

from __future__ import annotations

import copy
import hashlib
import json

from common import apply_change, change, delete, digest_strings
from control import (
    AGENT_STREAM,
    RUNTIME,
    STREAM,
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

SPEC = "docs/specs/journal.md v0.42 §9.6 (DEC-942)"
AT = "2026-09-21T14:10:00.000000000Z"
DECISION = "01J8Z3N9A000000000000000Q0"
REQUESTED = "01J8Z3P0A000000000000000Q1"
OUTPUT = "01J8Z3N8A000000000000000Q9"
BOUND = "sha256:" + "1" * 64
DEADLINE = 1790000100
DEADLINE_TEXT = "2026-09-21T14:15:00.000000000Z"
V1 = "requested_v1"
V2 = "requested_v2"
PLAN = {
    "asset_class": "us_equity",
    "tif": "gtc",
    "protection_required": True,
    "protection": {"stop": "147.25", "take_profit": "170.5"},
}


def oracle_hash(content: dict) -> str:
    """The oracle's own content hash: sorted keys, no whitespace, UTF-8 (journal spec §4), for the
    ASCII-only content these vectors hold."""
    text = json.dumps(content, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


def content(plan: dict | None) -> dict:
    out = {
        "action": {
            "instrument": "AAPL",
            "asset_class": "us_equity",
            "side": "buy",
            "qty": "10",
            "limit": "155",
            "order_usd": "1550",
            "purpose": "open",
        },
        "trigger": {"mandate_version": BOUND, "decided_by": "rule:large_order"},
        "evidence": {
            "combined_score": {"label": "combined model score, not a probability of profit", "value": "0.5"},
            "outputs": [{"event_id": OUTPUT, "artifact": None, "label": "Output of software you selected"}],
        },
        "risk_impact": [],
        "reference_mark": {"price": "155", "seq": 41},
        "deadline": DEADLINE_TEXT,
        "default": "If you do nothing, this action is skipped",
        "choices": ["approve", "skip"],
        "approvers": {"required": 1, "independent": False},
    }
    if plan is not None:
        out["execution"] = copy.deepcopy(plan)
    return out


def payload(body: dict) -> dict:
    return {
        "instrument": "AAPL",
        "asset_class": "us_equity",
        "side": "buy",
        "qty": "10",
        "limit": "155",
        "purpose": "open",
        "mandate_version": BOUND,
        "decided_by": "rule:large_order",
        "combined_score": "0.5",
        "reference_mark": {"price": "155", "seq": 41},
        "approvers_required": 1,
        "independent_required": False,
        "deadline": DEADLINE,
        "timeout_s": 300,
        "on_timeout": "skip",
        "content": body,
        "content_hash": oracle_hash(body),
    }


def draft(version: int, plan: dict | None) -> dict:
    p = payload(content(plan))
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": REQUESTED,
        "stream_id": AGENT_STREAM,
        "event_type": "ApprovalRequested",
        "schema_version": version,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": DECISION,
        "correlation_id": None,
        "actor": dict(RUNTIME),
        "config_refs": {"mandate_version": BOUND},
        "payload": p,
        "artifact_refs": sorted(digest_strings(p)),
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    return {V1: draft(1, None), V2: draft(2, PLAN)}


def restated(base: str, changes: list[dict]) -> list[dict]:
    """`changes`, then the content hash and the artifact references the changed draft states, so a
    draft that alters the content breaks only the rule it names."""
    after = copy.deepcopy(base_drafts()[base])
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
    return control_invalid(name, clause, base, changes, reason, path)


def valid(name, clause, base, changes):
    return control_valid(name, clause, base, changes)


EXECUTION = "payload.content.execution"

# Each plan member, a value of the wrong JSON kind (`schema`) and, where the type has a form, a
# string of the wrong form (`non_canonical`).
PLAN_CASES = (
    ("asset_class", 7, "equity"),
    ("tif", 7, "fok"),
    ("protection_required", "true", None),
    ("protection", "147.25", None),
)
PROTECTION_CASES = (
    ("stop", 147, "147.25.0"),
    ("take_profit", 170, "x"),
)
NULLED = ("asset_class", "tif", "protection_required")
# A null plan asset class is refused at its own path either way, as a type or by rule 136, so a
# validator that let it through its type is not a seeded bug these vectors can tell apart.


def plan_drafts() -> list[dict]:
    out = []
    for member, wrong_kind, wrong_form in PLAN_CASES:
        path = f"{EXECUTION}.{member}"
        out.append(invalid(f"plan.{member}.kind", "§9.6 v2 types", V2, restated(V2, [change(path, wrong_kind)]), "schema", path))
        if wrong_form is not None:
            out.append(
                invalid(f"plan.{member}.form", "§9.6 v2 types", V2, restated(V2, [change(path, wrong_form)]), "non_canonical", path)
            )
    for member, wrong_kind, wrong_form in PROTECTION_CASES:
        path = f"{EXECUTION}.protection.{member}"
        out.append(invalid(f"plan.{member}.kind", "§9.6 v2 types", V2, restated(V2, [change(path, wrong_kind)]), "schema", path))
        out.append(
            invalid(f"plan.{member}.form", "§9.6 v2 types", V2, restated(V2, [change(path, wrong_form)]), "non_canonical", path)
        )
    for member in NULLED:
        path = f"{EXECUTION}.{member}"
        out.append(invalid(f"plan.{member}.null", "§9.6 v2 types", V2, restated(V2, [change(path, None)]), "schema", path))
    stop = f"{EXECUTION}.protection.stop"
    out.append(invalid("plan.stop.null", "§9.6 v2 types", V2, restated(V2, [change(stop, None)]), "schema", stop))
    out.append(
        invalid(
            "plan.missing_member",
            "§9.6 v2 closed",
            V2,
            restated(V2, [delete(f"{EXECUTION}.protection_required")]),
            "schema",
            f"{EXECUTION}.protection_required",
        )
    )
    out.append(
        invalid(
            "plan.extra_member",
            "§9.6 v2 closed",
            V2,
            restated(V2, [change(f"{EXECUTION}.quantity", "10")]),
            "schema",
            f"{EXECUTION}.quantity",
        )
    )
    out.append(
        invalid(
            "plan.protection_missing_take_profit",
            "§9.6 v2 closed",
            V2,
            restated(V2, [delete(f"{EXECUTION}.protection.take_profit")]),
            "schema",
            f"{EXECUTION}.protection.take_profit",
        )
    )
    return out


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every plan member, rule 136, the
    version split, and §9.6's mirror, deadline, label, default, choices, and hash rules."""
    return [
        *plan_drafts(),
        invalid(
            "v2_without_a_plan",
            "§9.6 v2: the plan is required",
            V2,
            restated(V2, [delete(EXECUTION)]),
            "schema",
            EXECUTION,
        ),
        invalid(
            "v2_with_a_null_plan",
            "§9.6 v2: the plan is not nullable",
            V2,
            restated(V2, [change(EXECUTION, None)]),
            "schema",
            EXECUTION,
        ),
        invalid(
            "v1_with_a_plan",
            "§9.6 v1 closed: version 1 binds no plan",
            V1,
            restated(V1, [change(EXECUTION, copy.deepcopy(PLAN))]),
            "schema",
            EXECUTION,
        ),
        invalid(
            "plan_for_another_asset_class",
            "rule 136: the plan names the order's asset class",
            V2,
            restated(V2, [change(f"{EXECUTION}.asset_class", "crypto")]),
            "schema",
            f"{EXECUTION}.asset_class",
        ),
        invalid(
            "plan_changed_under_its_old_hash",
            "§9.6: the content hash covers the plan",
            V2,
            [change(f"{EXECUTION}.protection.stop", "140")],
            "schema",
            "payload.content_hash",
        ),
        invalid(
            "hash_of_the_content_without_its_plan",
            "§9.6: the content hash is the hash of the whole content",
            V2,
            [
                change("payload.content_hash", oracle_hash(content(None))),
                change(
                    "artifact_refs",
                    sorted({BOUND, oracle_hash(content(None))}),
                ),
            ],
            "schema",
            "payload.content_hash",
        ),
        invalid(
            "mirror_differs",
            "§9.6: every top-level member equals its content counterpart",
            V2,
            [change("payload.qty", "11")],
            "schema",
            "payload.qty",
        ),
        invalid(
            "deadline_differs",
            "§9.6: the top-level and content deadlines name one instant",
            V1,
            [change("payload.deadline", DEADLINE + 1)],
            "schema",
            "payload.deadline",
        ),
        invalid(
            "another_score_label",
            "§9.6: the score label is exact",
            V1,
            restated(V1, [change("payload.content.evidence.combined_score.label", "score")]),
            "schema",
            "payload.content.evidence.combined_score.label",
        ),
        invalid(
            "another_default",
            "§9.6: the default sentence is exact",
            V1,
            restated(V1, [change("payload.content.default", "Skipped if unanswered")]),
            "schema",
            "payload.content.default",
        ),
        invalid(
            "choices_reordered",
            "§9.6: the choices are exactly approve then skip",
            V1,
            restated(V1, [change("payload.content.choices", ["skip", "approve"])]),
            "schema",
            "payload.content.choices",
        ),
        invalid(
            "without_its_mandate",
            "§9 row: `ApprovalRequested` names `mandate_version`",
            V2,
            [change("config_refs", {})],
            "missing_config_ref",
            "config_refs.mandate_version",
        ),
        invalid(
            "on_the_control_stream",
            "§9 row: `ApprovalRequested` is an agent-stream record",
            V2,
            [change("stream_id", STREAM)],
            "wrong_stream",
            "event_type",
        ),
        invalid(
            "at_version_4",
            "§9.6: versions 1 to 3 only (v0.88)",
            V2,
            [change("schema_version", 4)],
            "unknown_schema",
            "payload",
        ),
    ]


def valid_drafts() -> list[dict]:
    """Cases a rule might be misread to refuse; each must be accepted."""
    return [
        valid(
            "plan_without_protection",
            "§9.6 v2: protection is nullable when the mandate disables it",
            V2,
            restated(
                V2,
                [change(f"{EXECUTION}.protection_required", False), change(f"{EXECUTION}.protection", None)],
            ),
        ),
        valid(
            "plan_without_a_take_profit",
            "§9.6 v2: the take-profit is nullable",
            V2,
            restated(V2, [change(f"{EXECUTION}.protection.take_profit", None), change(f"{EXECUTION}.tif", "day")]),
        ),
        valid(
            "crypto_order_and_plan",
            "rule 136: any asset class, so long as the plan and the order agree",
            V2,
            restated(
                V2,
                [
                    change("payload.asset_class", "crypto"),
                    change("payload.content.action.asset_class", "crypto"),
                    change(f"{EXECUTION}.asset_class", "crypto"),
                    change(f"{EXECUTION}.tif", "ioc"),
                ],
            ),
        ),
        valid(
            "v1_without_a_mark",
            "§9.6: the reference mark is nullable at either version",
            V1,
            restated(V1, [change("payload.reference_mark", None), change("payload.content.reference_mark", None)]),
        ),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = ("drafts.valid", "drafts.hash", "drafts.plan", "invalid_drafts", "valid_drafts")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, base in section["drafts"].items():
        got = violations(base)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
        stated = base["payload"]["content_hash"]
        if stated != oracle_hash(base["payload"]["content"]):
            problems.append(found("drafts.hash", f"base draft {name} states another hash than its content's"))
    v1, v2 = section["drafts"][V1], section["drafts"][V2]
    if v1["schema_version"] != 1 or "execution" in v1["payload"]["content"]:
        problems.append(found("drafts.plan", "the version 1 base binds a plan"))
    if v2["schema_version"] != 2 or v2["payload"]["content"].get("execution") != PLAN:
        problems.append(found("drafts.plan", "the version 2 base does not bind the builder's plan"))
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
    "rule.136",
    "rule.9.6.content_hash",
    "rule.9.6.deadline",
    "rule.9.6.label",
    "rule.9.6.default",
    "rule.9.6.choices",
    "rule.9.6.mirror.qty",
    "record.extra",
    "record.missing",
    "config_refs.required",
    *(f"loose.payload.{member}" for member, _, _ in PLAN_CASES),
    *(f"loose.payload.{member}" for member, _, _ in PROTECTION_CASES),
    *(f"nullable.payload.{member}" for member in NULLED if member != "asset_class"),
    "nullable.payload.stop",
)


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    def replan(s):
        base = s["drafts"][V2]
        base["payload"]["content"]["execution"]["tif"] = "day"
        base["payload"]["content_hash"] = oracle_hash(base["payload"]["content"])
        base["artifact_refs"] = sorted(digest_strings(base["payload"]))

    return [
        (
            "the version 2 base names another asset class in its plan",
            "drafts.valid",
            mutated(lambda s: s["drafts"][V2]["payload"]["content"]["execution"].update(asset_class="crypto")),
        ),
        (
            "the version 1 base states a hash its content does not have",
            "drafts.hash",
            mutated(lambda s: s["drafts"][V1]["payload"].update(content_hash=BOUND)),
        ),
        (
            "the version 2 base binds another plan than the builder's",
            "drafts.plan",
            mutated(replan),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "plan_for_another_asset_class")["expect"].update(path="payload.asset_class")),
        ),
        (
            "a valid draft changes its plan under the old hash",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "plan_without_a_take_profit")["changes"].pop()),
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
        for d, want in cases:
            got = violations(d, skip)
            caught |= bool(got) if want is None else not reported(got, want)
        if not caught:
            escaped.append(f"request validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"request check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"request vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
