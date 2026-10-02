"""Journal spec v0.8 §9.3's reference vectors (DEC-403): the account stream's `MandateVersionApplied`
and `UniverseChanged`.

The schemas and rules 28 to 31 live in `control.py`, beside §9.2's, so one validator judges every
closed schema. This module builds the `risk_state` section: the two stored mandate documents a
version change names, base drafts of both records on the account stream, the `JournaledFact` each
maps to, an invalid draft for every rule and member type, and valid drafts for the cases a rule might
be misread to refuse. It checks the section with its own oracles and shows every seeded bug caught.
"""

from __future__ import annotations

import copy

from common import artifact_ref, change, delete, is_ulid, normalize_decimal
from control import (
    ACCOUNT_STREAM_REF,
    AGENT,
    EXECUTOR,
    WORKSPACE,
    canonical_of,
    check_of,
    draft_for,
    found,
    invalid,
    mandate_document,
    reported,
    sha,
    step_up,
    valid,
    violations,
)

SPEC = "docs/specs/journal.md v0.8 §9.3 (DEC-403)"
ACCOUNT = f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}"
RESEARCHER = "agent_b"
PINNED_ASSET = "7b4a1c2e-1111-4a2b-9c3d-000000000001"
ADMITTED_ASSET = "7b4a1c2e-2222-4a2b-9c3d-000000000002"
THESIS = "thesis_01J8ZT"
LINEAGE = "lineage_01J8ZT"
BASE_IDS = {
    "version_applied": "01J8ZRA0A000000000000000V1",
    "version_rejected": "01J8ZRA1A000000000000000V2",
    "universe_admitted": "01J8ZRB0A000000000000000V3",
    "universe_removed_pinned": "01J8ZRB1A000000000000000V4",
}
assert all(is_ulid(i) for i in BASE_IDS.values()), "event IDs (§3)"
NEW_ALLOCATION = "12500"


def documents() -> dict[str, dict]:
    """The version in force and the one applied after it: the same mandate with its allocation
    raised, which mandate spec §9.2 classifies as risk-increasing."""
    old = mandate_document()
    new = copy.deepcopy(old)
    new["capital"]["allocation_usd"] = NEW_ALLOCATION
    return {"old_document": old, "new_document": new}


def base_drafts() -> dict[str, dict]:
    docs = documents()
    old, new = artifact_ref(docs["old_document"]), artifact_ref(docs["new_document"])

    def draft(name, event_type, at, payload, version):
        return {
            "envelope_version": 1,
            "environment": "paper",
            "event_id": BASE_IDS[name],
            "stream_id": ACCOUNT,
            "event_type": event_type,
            "schema_version": 1,
            "event_time": at,
            "clock_source": "local",
            "causation_id": None,
            "correlation_id": None,
            "actor": dict(EXECUTOR),
            "config_refs": {"mandate_version": version},
            "payload": payload,
            "artifact_refs": sorted({old, new} & {v for v in payload.values() if isinstance(v, str)}),
            "pii_refs": [],
        }

    applied_at = "2026-09-22T14:30:00.000000000Z"
    return {
        "version_applied": draft(
            "version_applied",
            "MandateVersionApplied",
            applied_at,
            {
                "agent_id": AGENT,
                "old_version": old,
                "new_version": new,
                "classification": "risk_increasing",
                "step_up": step_up("2026-09-22T14:29:10.000000000Z"),
                "result": "applied",
                "reason": None,
                "allocation_change": "2500",
                "max_loss_from_allocation": None,
                "risk_clock": "2026-09-22T14:30:00.000000000Z",
            },
            new,
        ),
        "version_rejected": draft(
            "version_rejected",
            "MandateVersionApplied",
            "2026-09-22T15:00:00.000000000Z",
            {
                "agent_id": AGENT,
                "old_version": old,
                "new_version": new,
                "classification": "risk_increasing",
                "step_up": step_up("2026-09-22T14:59:20.000000000Z"),
                "result": "rejected",
                "reason": "increase_blocked_while_latched",
                "allocation_change": None,
                "max_loss_from_allocation": None,
                "risk_clock": "2026-09-22T15:00:00.000000000Z",
            },
            old,
        ),
        "universe_admitted": draft(
            "universe_admitted",
            "UniverseChanged",
            "2026-09-22T16:00:00.000000000Z",
            {
                "agent_id": RESEARCHER,
                "instrument": ADMITTED_ASSET,
                "change": "admitted",
                "reason": "thesis_admitted",
                "thesis_id": THESIS,
                "lineage_id": LINEAGE,
                "universe_size_after": 1,
                "risk_clock": "2026-09-22T16:00:00.000000000Z",
            },
            old,
        ),
        "universe_removed_pinned": draft(
            "universe_removed_pinned",
            "UniverseChanged",
            "2026-09-22T16:30:00.000000000Z",
            {
                "agent_id": AGENT,
                "instrument": PINNED_ASSET,
                "change": "removed",
                "reason": "version_applied",
                "thesis_id": None,
                "lineage_id": None,
                "universe_size_after": 0,
                "risk_clock": "2026-09-22T16:30:00.000000000Z",
            },
            new,
        ),
    }


def expected_facts() -> list[dict]:
    """What each base draft maps to, written from the constants the drafts were built from."""
    old = mandate_document()
    return [
        {
            "draft": "version_applied",
            "fact": {
                "kind": "AgentVersionActive",
                "agent": AGENT,
                "connection_id": old["connection_id"],
                "environment": "paper",
                "allocation_usd": NEW_ALLOCATION,
                "pinned": [PINNED_ASSET],
            },
        },
        {"draft": "version_rejected", "fact": None},
        {
            "draft": "universe_admitted",
            "fact": {"kind": "UniverseChanged", "agent": RESEARCHER, "instrument": ADMITTED_ASSET, "admitted": True},
        },
        {
            "draft": "universe_removed_pinned",
            "fact": {"kind": "UniverseChanged", "agent": AGENT, "instrument": PINNED_ASSET, "admitted": False},
        },
    ]


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule, or the rules its `also` lists; together they cover every
    §9.3 member type and rule."""
    docs = documents()
    old = artifact_ref(docs["old_document"])
    return [
        invalid(
            "applied_carries_a_comment",
            "§9.3 closed schema",
            "version_applied",
            [change("payload.comment", "raised for the quarter")],
            "schema",
            "payload.comment",
        ),
        invalid(
            "applied_without_risk_clock",
            "§9.1 absent member",
            "version_applied",
            [delete("payload.risk_clock")],
            "schema",
            "payload.risk_clock",
        ),
        invalid(
            "applied_risk_clock_off_the_second",
            "§9.2 risk_clock",
            "version_applied",
            [change("payload.risk_clock", "2026-09-22T14:30:00.500000000Z")],
            "non_canonical",
            "payload.risk_clock",
        ),
        invalid(
            "applied_old_version_a_bare_hash",
            "§9.1 ref",
            "version_applied",
            [change("payload.old_version", old.removeprefix("sha256:")), change("artifact_refs", [artifact_ref(docs["new_document"])])],
            "non_canonical",
            "payload.old_version",
        ),
        invalid(
            "applied_new_version_null",
            "§9.3 new_version: never null",
            "version_applied",
            [change("payload.new_version", None), change("artifact_refs", [old])],
            "schema",
            "payload.new_version",
        ),
        invalid(
            "applied_classified_invalid",
            "§9.3 classification: an invalid change never applies",
            "version_applied",
            [change("payload.classification", "invalid")],
            "non_canonical",
            "payload.classification",
        ),
        invalid(
            "applied_result_pending",
            "§9.3 result",
            "version_applied",
            [change("payload.result", "pending")],
            "non_canonical",
            "payload.result",
        ),
        invalid(
            "applied_allocation_change_not_a_decimal",
            "§9.1 decimal",
            "version_applied",
            [change("payload.allocation_change", "2,500")],
            "non_canonical",
            "payload.allocation_change",
        ),
        invalid(
            "applied_step_up_carries_a_token",
            "§9.3 step_up: closed",
            "version_applied",
            [change("payload.step_up.token", "secret")],
            "schema",
            "payload.step_up.token",
        ),
        invalid(
            "increasing_without_step_up",
            "rule 28",
            "version_applied",
            [change("payload.step_up", None)],
            "schema",
            "payload.step_up",
        ),
        invalid(
            "rejected_without_reason",
            "rule 29",
            "version_rejected",
            [change("payload.reason", None)],
            "schema",
            "payload.reason",
        ),
        invalid(
            "applied_with_a_reason",
            "rule 29",
            "version_applied",
            [change("payload.reason", "would_trigger_limit")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "rejected_with_an_allocation_change",
            "rule 29",
            "version_rejected",
            [change("payload.allocation_change", "2500")],
            "schema",
            "payload.allocation_change",
        ),
        invalid(
            "rejected_with_a_floor",
            "rule 29",
            "version_rejected",
            [change("payload.max_loss_from_allocation", "0.15")],
            "schema",
            "payload.max_loss_from_allocation",
        ),
        invalid(
            "rejected_reason_unknown",
            "§9.3 reason",
            "version_rejected",
            [change("payload.reason", "nothing_to_acknowledge")],
            "non_canonical",
            "payload.reason",
        ),
        invalid(
            "rejected_without_reason_and_with_allocation",
            "rule 29: the first offending member, reason before allocation_change",
            "version_rejected",
            [change("payload.reason", None), change("payload.allocation_change", "2500")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "applied_without_mandate_ref",
            "§9 required config_refs",
            "version_applied",
            [change("config_refs", {})],
            "missing_config_ref",
            "config_refs.mandate_version",
        ),
        invalid(
            "admitted_instrument_not_an_id",
            "§9.1 id",
            "universe_admitted",
            [change("payload.instrument", "BTC/USD")],
            "non_canonical",
            "payload.instrument",
        ),
        invalid(
            "admitted_size_as_text",
            "§9.1 integer",
            "universe_admitted",
            [change("payload.universe_size_after", "1")],
            "schema",
            "payload.universe_size_after",
        ),
        invalid(
            "admitted_change_paused",
            "§9.3 change",
            "universe_admitted",
            [change("payload.change", "paused")],
            "non_canonical",
            "payload.change",
        ),
        invalid(
            "admitted_reason_unknown",
            "§9.3 reason",
            "universe_admitted",
            [change("payload.reason", "owner_added")],
            "non_canonical",
            "payload.reason",
        ),
        invalid(
            "thesis_admission_removes",
            "rule 30",
            "universe_admitted",
            [change("payload.change", "removed")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "expiry_admits",
            "rule 30",
            "universe_admitted",
            [change("payload.reason", "thesis_expired")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "admitted_without_thesis",
            "rule 31: null together, at the null one",
            "universe_admitted",
            [change("payload.thesis_id", None)],
            "schema",
            "payload.thesis_id",
        ),
        invalid(
            "admitted_without_lineage",
            "rule 31: null together, at the null one",
            "universe_admitted",
            [change("payload.lineage_id", None)],
            "schema",
            "payload.lineage_id",
        ),
        invalid(
            "admitted_with_neither",
            "rule 31: a thesis reason names its thesis",
            "universe_admitted",
            [change("payload.thesis_id", None), change("payload.lineage_id", None)],
            "schema",
            "payload.thesis_id",
        ),
        invalid(
            "halt_without_thesis",
            "rule 31: an operator halts per thesis",
            "universe_removed_pinned",
            [change("payload.reason", "operator_halt")],
            "schema",
            "payload.thesis_id",
        ),
        invalid(
            "pinned_removal_names_a_thesis",
            "rule 31: a pinned list names no thesis",
            "universe_removed_pinned",
            [change("payload.thesis_id", THESIS), change("payload.lineage_id", LINEAGE)],
            "schema",
            "payload.thesis_id",
        ),
        invalid(
            "removed_on_the_control_stream",
            "§9 stream",
            "universe_removed_pinned",
            [change("stream_id", f"ctl:{WORKSPACE}")],
            "wrong_stream",
            "event_type",
        ),
    ]


def valid_drafts() -> list[dict]:
    return [
        valid(
            "neutral_without_step_up",
            "rule 28: only a risk-increasing version needs step-up",
            "version_applied",
            [change("payload.classification", "neutral"), change("payload.step_up", None)],
        ),
        valid(
            "reducing_with_step_up",
            "rule 28: step-up is allowed on any version",
            "version_applied",
            [change("payload.classification", "risk_reducing")],
        ),
        valid(
            "applied_changing_neither",
            "rule 29: an applied version may change neither the allocation nor the floor",
            "version_applied",
            [change("payload.allocation_change", None)],
        ),
        valid(
            "floor_loosened",
            "rule 29: an applied floor-loosening version",
            "version_applied",
            [change("payload.allocation_change", None), change("payload.max_loss_from_allocation", "0.15")],
        ),
        valid(
            "pinned_instrument_admitted",
            "rule 30: version_applied admits",
            "universe_removed_pinned",
            [change("payload.change", "admitted"), change("payload.universe_size_after", 1)],
        ),
        valid(
            "eligibility_lost_on_a_pinned_instrument",
            "rule 31: eligibility_lost may name no thesis",
            "universe_removed_pinned",
            [change("payload.reason", "eligibility_lost")],
        ),
        valid(
            "eligibility_lost_on_a_thesis",
            "rule 31: eligibility_lost may name its thesis",
            "universe_admitted",
            [change("payload.change", "removed"), change("payload.reason", "eligibility_lost"), change("payload.universe_size_after", 0)],
        ),
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = (
    "artifacts.rehash",
    "artifacts.missing",
    "drafts.valid",
    "facts.recompute",
    "invalid_drafts",
    "valid_drafts",
)


def recompute_facts(section: dict) -> list[dict]:
    """The `JournaledFact` each base draft maps to, from its payload and the stored documents alone."""
    stored = {a["ref"]: a["object"] for a in section["artifacts"]}
    out = []
    for name, draft in section["drafts"].items():
        p, fact = draft["payload"], None
        match draft["event_type"]:
            case "MandateVersionApplied" if p["result"] == "applied" and p["new_version"] not in stored:
                fact = {"kind": "unresolved", "new_version": p["new_version"]}
            case "MandateVersionApplied" if p["result"] == "applied":
                document = stored[p["new_version"]]
                fact = {
                    "kind": "AgentVersionActive",
                    "agent": p["agent_id"],
                    "connection_id": document["connection_id"],
                    "environment": document["environment"],
                    "allocation_usd": normalize_decimal(document["capital"]["allocation_usd"]),
                    "pinned": sorted(i["asset_id"] for i in document["universe"]["pinned_instruments"]),
                }
            case "UniverseChanged":
                fact = {
                    "kind": "UniverseChanged",
                    "agent": p["agent_id"],
                    "instrument": p["instrument"],
                    "admitted": p["change"] == "admitted",
                }
        out.append({"draft": name, "fact": fact})
    return out


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    stored = {a["ref"]: a for a in section["artifacts"]}
    for a in section["artifacts"]:
        if canonical_of(a["object"]) != a["canonical"] or "sha256:" + sha(a["canonical"]) != a["ref"]:
            problems.append(found("artifacts.rehash", f"artifact {a['name']}: does not re-hash"))
    for name, draft in section["drafts"].items():
        for ref in draft["artifact_refs"]:
            if ref not in stored:
                problems.append(found("artifacts.missing", f"draft {name}: artifact {ref} missing"))
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    if recompute_facts(section) != section["journaled_facts"]:
        problems.append(found("facts.recompute", "the listed facts are not what the drafts map to"))
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
    "rule.28",
    "rule.29",
    "rule.29.reason",
    "rule.30",
    "rule.31",
    "rule.31.thesis",
    "rule.31.pinned",
    "loose.payload.old_version",
    "loose.payload.instrument",
    "loose.payload.classification",
    "loose.payload.universe_size_after",
    "open.step_up",
)


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def artifact(s, name):
        return next(a for a in s["artifacts"] if a["name"] == name)

    return [
        (
            "an artifact's object edited",
            "artifacts.rehash",
            mutated(lambda s: artifact(s, "new_document")["object"]["capital"].update(allocation_usd="13000")),
        ),
        (
            "a base draft names a document not stored",
            "artifacts.missing",
            mutated(lambda s: s["drafts"]["universe_admitted"]["artifact_refs"].append("sha256:" + "e" * 64)),
        ),
        (
            "a base draft breaks rule 28",
            "drafts.valid",
            mutated(lambda s: s["drafts"]["version_rejected"]["payload"].update(step_up=None)),
        ),
        (
            "a listed fact differs",
            "facts.recompute",
            mutated(lambda s: s["journaled_facts"][2]["fact"].update(admitted=False)),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: s["invalid_drafts"][0]["expect"].update(path="payload.note")),
        ),
        (
            "a valid draft breaks a rule",
            "valid_drafts",
            mutated(lambda s: s["valid_drafts"][0]["changes"].append(change("payload.result", "rejected"))),
        ),
    ]


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught; a vector mutant only by the check it is registered against, with no
    other check of that check's family also catching it."""
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
            escaped.append(f"risk-state validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"risk-state check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        family = check.partition(".")[0]
        masked = sorted(c for c in caught_by if c != check and c.partition(".")[0] == family)
        if check not in caught_by:
            escaped.append(f"risk-state vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
        elif masked:
            escaped.append(f"risk-state vector mutant: {name} (caught by {check} and also by {masked})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    docs = documents()
    return {
        "spec": SPEC,
        "stream_id": ACCOUNT,
        "artifacts": [
            {"name": name, "ref": artifact_ref(obj), "object": obj, "canonical": canonical_of(obj)}
            for name, obj in docs.items()
        ],
        "drafts": base_drafts(),
        "journaled_facts": expected_facts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }

