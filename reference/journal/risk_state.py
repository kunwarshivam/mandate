"""Journal spec v0.8 §9.3's reference vectors (DEC-403): the account stream's `MandateVersionApplied`
and `UniverseChanged`.

The schemas and rules 29 to 33 live in `control.py`, beside §9.2's, so one validator judges every
closed schema. This module builds the `risk_state` section: the stored mandate documents the records
name, base drafts of both records on the account stream, the `JournaledFact` each maps to, an invalid
draft for every rule and member type, and valid drafts for the cases a rule might be misread to
refuse. It checks the section with its own oracles, including that every stored document is a valid
mandate and that every base and valid draft states the classification mandate spec §9.2 gives its two
documents, and shows every seeded bug caught.
"""

from __future__ import annotations

import copy
import functools
from decimal import Decimal

import yaml

from common import artifact_ref, change, delete, is_ulid, normalize_decimal
from control import (
    ACCOUNT_STREAM_REF,
    AGENT,
    EXECUTOR,
    MANDATE_CASES,
    WORKSPACE,
    canonical_of,
    check_of,
    draft_for,
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
RESEARCH_BASE = "research_equity"


@functools.cache
def proven_research_document() -> dict:
    """The mandate reference cases' `research_equity`, after re-hashing it to the canonical hash that
    file records, so the stored document is the proven base: unpinned, with a research agent and an
    admitting model, so a thesis can admit under it (MI-20 keeps one out of a pinned universe)."""
    cases = yaml.safe_load(MANDATE_CASES.read_text(encoding="utf-8"))
    base = cases["bases"][RESEARCH_BASE]
    assert artifact_ref(base["mandate"]) == base["canonical_sha256"], "the research base re-hashes"
    return base["mandate"]


def research_document() -> dict:
    return copy.deepcopy(proven_research_document())


# Each version after the one in force changes exactly one path of it, and mandate spec §9.2 classifies
# that path by its row: a maximum larger is risk-increasing and smaller risk-reducing, and `name` is
# neutral. `documents.valid` and `drafts.classification` check the stored documents against these.
VERSION_PATHS = {
    "new_document": ("capital/allocation_usd", NEW_ALLOCATION),
    "renamed_document": ("name", "btc-accumulator-renamed"),
    "tightened_document": ("capital/max_loss_from_allocation", "0.08"),
    "loosened_document": ("capital/max_loss_from_allocation", "0.15"),
}
MAXIMUMS = ("capital/allocation_usd", "capital/max_loss_from_allocation")
NEUTRAL = ("name",)


def documents() -> dict[str, dict]:
    """The version in force (`old_document`), four versions after it that mandate spec §9.2
    classifies as risk-increasing (allocation or floor raised), neutral (renamed), and risk-reducing
    (floor lowered), and the research agent's mandate."""
    old = mandate_document()

    def changed(path: str, value) -> dict:
        new = copy.deepcopy(old)
        *parents, last = path.split("/")
        node = new
        for name in parents:
            node = node[name]
        node[last] = value
        return new

    return {
        "old_document": old,
        **{name: changed(path, value) for name, (path, value) in VERSION_PATHS.items()},
        "research_document": research_document(),
    }


def refs() -> dict[str, str]:
    return {name: artifact_ref(doc) for name, doc in documents().items()}


def base_drafts() -> dict[str, dict]:
    r = refs()
    old, new, research = r["old_document"], r["new_document"], r["research_document"]

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
            "artifact_refs": sorted({v for v in payload.values() if isinstance(v, str) and v.startswith("sha256:")}),
            "pii_refs": [],
        }

    return {
        "version_applied": draft(
            "version_applied",
            "MandateVersionApplied",
            "2026-09-22T14:30:00.000000000Z",
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
            research,
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


def versions(old: str, new: str) -> list[dict]:
    """The changes that name another pair of stored documents, with `artifact_refs` to match."""
    return [
        change("payload.old_version", old),
        change("payload.new_version", new),
        change("artifact_refs", sorted({old, new})),
    ]


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule, or the rules its `also` lists; together they cover every
    §9.3 member type and rule."""
    r = refs()
    old = r["old_document"]
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
            "applied_agent_not_an_id",
            "§9.1 id",
            "version_applied",
            [change("payload.agent_id", "agent a")],
            "non_canonical",
            "payload.agent_id",
        ),
        invalid(
            "applied_old_version_a_bare_hash",
            "§9.1 ref",
            "version_applied",
            [change("payload.old_version", old.removeprefix("sha256:")), change("artifact_refs", [r["new_document"]])],
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
            "loosened_floor_as_a_percentage",
            "§9.1 decimal",
            "version_applied",
            [*versions(old, r["loosened_document"]), change("payload.allocation_change", None),
             change("payload.max_loss_from_allocation", "15%")],
            "non_canonical",
            "payload.max_loss_from_allocation",
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
            "rule 29",
            "version_applied",
            [change("payload.step_up", None)],
            "schema",
            "payload.step_up",
        ),
        invalid(
            "rejected_without_reason",
            "rule 30",
            "version_rejected",
            [change("payload.reason", None)],
            "schema",
            "payload.reason",
        ),
        invalid(
            "applied_with_a_reason",
            "rule 30",
            "version_applied",
            [change("payload.reason", "would_trigger_limit")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "rejected_with_an_allocation_change",
            "rule 30",
            "version_rejected",
            [change("payload.allocation_change", "2500")],
            "schema",
            "payload.allocation_change",
        ),
        invalid(
            "rejected_with_a_floor",
            "rule 30",
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
            "rule 30: the first offending member, reason before allocation_change",
            "version_rejected",
            [change("payload.reason", None), change("payload.allocation_change", "2500")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "raise_classified_neutral",
            "rule 33: an applied allocation increase is risk-increasing",
            "version_applied",
            [change("payload.classification", "neutral")],
            "schema",
            "payload.classification",
        ),
        invalid(
            "floor_raise_classified_reducing",
            "rule 33: an applied floor raise is risk-increasing",
            "version_applied",
            [*versions(old, r["loosened_document"]), change("payload.classification", "risk_reducing"),
             change("payload.allocation_change", None), change("payload.max_loss_from_allocation", "0.15")],
            "schema",
            "payload.classification",
        ),
        invalid(
            "latched_increase_classified_neutral",
            "rule 33: a latched increase is refused only on a risk-increasing version",
            "version_rejected",
            [change("payload.classification", "neutral")],
            "schema",
            "payload.classification",
        ),
        invalid(
            "raise_classified_neutral_without_step_up",
            "rules 29 and 33: a misclassified raise is reported at its classification",
            "version_applied",
            [change("payload.classification", "neutral"), change("payload.step_up", None)],
            "schema",
            "payload.classification",
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
            "§9.3 asset_id",
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
            "admitted_thesis_not_an_id",
            "§9.1 id",
            "universe_admitted",
            [change("payload.thesis_id", "thesis 01")],
            "non_canonical",
            "payload.thesis_id",
        ),
        invalid(
            "admitted_lineage_not_an_id",
            "§9.1 id",
            "universe_admitted",
            [change("payload.lineage_id", "lineage/01")],
            "non_canonical",
            "payload.lineage_id",
        ),
        invalid(
            "admitted_instrument_an_id_not_an_asset_id",
            "§9.3 asset_id: an id that is not the uuid form (#497 round 1, m3)",
            "universe_admitted",
            [change("payload.instrument", "BTCUSD")],
            "non_canonical",
            "payload.instrument",
        ),
        invalid(
            "admitted_instrument_not_a_string",
            "§9.3 asset_id: not a string (#509 round 1, m2)",
            "universe_admitted",
            [change("payload.instrument", 2)],
            "schema",
            "payload.instrument",
        ),
        invalid(
            "admitted_instrument_with_a_trailing_newline",
            "§9.3 asset_id: the whole string, so a trailing newline is refused (#511 round 1)",
            "universe_admitted",
            [change("payload.instrument", ADMITTED_ASSET + "\n")],
            "non_canonical",
            "payload.instrument",
        ),
        invalid(
            "admitted_instrument_in_capitals",
            "§9.3 asset_id: lowercase only, so one asset has one spelling",
            "universe_admitted",
            [change("payload.instrument", ADMITTED_ASSET.upper())],
            "non_canonical",
            "payload.instrument",
        ),
        invalid(
            "order_29_before_30",
            "rules 29 then 30: a risk-increasing applied record without step-up and with a reason",
            "version_applied",
            [change("payload.step_up", None), change("payload.reason", "would_trigger_limit")],
            "schema",
            "payload.step_up",
            also=[("schema", "payload.reason")],
        ),
        invalid(
            "order_30_before_33",
            "rules 30 then 33: a rejected raise labelled neutral that also carries an allocation change",
            "version_rejected",
            [change("payload.classification", "neutral"), change("payload.allocation_change", "2500")],
            "schema",
            "payload.allocation_change",
            also=[("schema", "payload.classification")],
        ),
        invalid(
            "order_31_before_32",
            "rules 31 then 32: a thesis admission that removes and names no thesis",
            "universe_admitted",
            [change("payload.change", "removed"), change("payload.thesis_id", None),
             change("payload.lineage_id", None), change("payload.universe_size_after", 0)],
            "schema",
            "payload.reason",
            also=[("schema", "payload.thesis_id")],
        ),
        invalid(
            "thesis_admission_removes",
            "rule 31",
            "universe_admitted",
            [change("payload.change", "removed")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "expiry_admits",
            "rule 31",
            "universe_admitted",
            [change("payload.reason", "thesis_expired")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "admitted_without_thesis",
            "rule 32: null together, at the null one",
            "universe_admitted",
            [change("payload.thesis_id", None)],
            "schema",
            "payload.thesis_id",
        ),
        invalid(
            "admitted_without_lineage",
            "rule 32: null together, at the null one",
            "universe_admitted",
            [change("payload.lineage_id", None)],
            "schema",
            "payload.lineage_id",
        ),
        invalid(
            "admitted_with_neither",
            "rule 32: a thesis reason names its thesis",
            "universe_admitted",
            [change("payload.thesis_id", None), change("payload.lineage_id", None)],
            "schema",
            "payload.thesis_id",
        ),
        invalid(
            "halt_without_thesis",
            "rule 32: an operator halts per thesis",
            "universe_removed_pinned",
            [change("payload.reason", "operator_halt")],
            "schema",
            "payload.thesis_id",
        ),
        invalid(
            "pinned_admission_names_a_thesis",
            "rule 32: an instrument a pinned list admits follows from no thesis",
            "universe_removed_pinned",
            [change("payload.change", "admitted"), change("payload.universe_size_after", 1),
             change("payload.thesis_id", THESIS), change("payload.lineage_id", LINEAGE)],
            "schema",
            "payload.thesis_id",
        ),
        invalid(
            "pinning_switch_removal_with_thesis_alone",
            "rule 32: null together, at the null one",
            "universe_removed_pinned",
            [change("payload.thesis_id", THESIS)],
            "schema",
            "payload.lineage_id",
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
    """Each is a state mandate spec §9.2 and the risk fold can produce, and asserts what its title
    says: the documents it names classify as it states."""
    r = refs()
    old = r["old_document"]
    return [
        valid(
            "neutral_without_step_up",
            "rule 29: only a risk-increasing version needs step-up",
            "version_applied",
            [*versions(old, r["renamed_document"]), change("payload.classification", "neutral"),
             change("payload.step_up", None), change("payload.allocation_change", None)],
        ),
        valid(
            "reducing_with_step_up",
            "rule 29: step-up is allowed on any version",
            "version_applied",
            [*versions(old, r["tightened_document"]), change("payload.classification", "risk_reducing"),
             change("payload.allocation_change", None)],
        ),
        valid(
            "applied_changing_neither",
            "rule 30: an applied version may change neither the allocation nor the floor",
            "version_applied",
            [*versions(old, r["renamed_document"]), change("payload.classification", "neutral"),
             change("payload.allocation_change", None)],
        ),
        valid(
            "floor_loosened",
            "rules 30 and 33: an applied floor raise, risk-increasing with step-up",
            "version_applied",
            [*versions(old, r["loosened_document"]), change("payload.allocation_change", None),
             change("payload.max_loss_from_allocation", "0.15")],
        ),
        valid(
            "pinned_instrument_admitted",
            "rule 31: version_applied admits",
            "universe_removed_pinned",
            [change("payload.change", "admitted"), change("payload.universe_size_after", 1)],
        ),
        valid(
            "pinning_switch_removes_a_thesis_instrument",
            "rule 32: DEC-121's pinning switch removes an admitted instrument and names its thesis",
            "universe_admitted",
            [change("payload.change", "removed"), change("payload.reason", "version_applied"),
             change("payload.universe_size_after", 0)],
        ),
        valid(
            "eligibility_lost_on_a_pinned_instrument",
            "rule 32: eligibility_lost may name no thesis",
            "universe_removed_pinned",
            [change("payload.reason", "eligibility_lost")],
        ),
        valid(
            "eligibility_lost_on_a_thesis",
            "rule 32: eligibility_lost may name its thesis",
            "universe_admitted",
            [change("payload.change", "removed"), change("payload.reason", "eligibility_lost"),
             change("payload.universe_size_after", 0)],
        ),
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = (
    "artifacts.rehash",
    "artifacts.missing",
    "documents.valid",
    "drafts.valid",
    "drafts.classification",
    "drafts.thesis_under_research",
    "facts.recompute",
    "invalid_drafts",
    "valid_drafts",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def differing_paths(old, new, prefix: str = "") -> list[str]:
    """The leaf paths two documents differ at, `/`-joined; a list differs as a whole."""
    if isinstance(old, dict) and isinstance(new, dict):
        out = []
        for key in sorted(set(old) | set(new)):
            path = f"{prefix}/{key}".lstrip("/")
            if key not in old or key not in new:
                out.append(path)
            else:
                out += differing_paths(old[key], new[key], path)
        return out
    return [] if old == new else [prefix]


def classify(old: dict, new: dict) -> str:
    """Mandate spec §9.2's verdict for the paths these fixtures change, computed here and not taken
    from the drafts: any other path is refused rather than guessed (fail closed)."""
    verdicts = set()
    for path in differing_paths(old, new):
        if path in MAXIMUMS:
            a, b = (Decimal(doc_at(d, path)) for d in (old, new))
            verdicts.add("risk_increasing" if b > a else "risk_reducing" if b < a else "neutral")
        elif path in NEUTRAL:
            verdicts.add("neutral")
        else:
            raise ValueError(f"no §9.2 row is encoded here for {path}")
    for verdict in ("risk_increasing", "risk_reducing"):
        if verdict in verdicts:
            return verdict
    return "neutral"


def doc_at(document: dict, path: str):
    node = document
    for name in path.split("/"):
        node = node[name]
    return node


def document_problems(name: str, document: dict, base: dict) -> list[str]:
    """A stored version is the proven base changed at its one listed path, to a value that keeps the
    mandate's own bounds: the risk maximums under the allocation (V-013) and the floor a fraction in
    (0, 1]. The research document is the proven `research_equity` base itself."""
    if name == "research_document":
        return [] if document == proven_research_document() else [f"{name} is not the proven research base"]
    if name == "old_document":
        return [] if document == base else [f"{name} is not the control-stream vectors' mandate"]
    path, value = VERSION_PATHS[name]
    problems = []
    if differing_paths(base, document) != [path] or doc_at(document, path) != value:
        problems.append(f"{name} does not change exactly {path} to {value}")
    risk, capital = document["risk"], document["capital"]
    order = [risk["max_order_usd"], risk["max_position_usd"], risk["max_gross_exposure_usd"], capital["allocation_usd"]]
    if [Decimal(x) for x in order] != sorted(Decimal(x) for x in order):
        problems.append(f"{name} breaks V-013's order")
    if not Decimal("0") < Decimal(capital["max_loss_from_allocation"]) <= Decimal("1"):
        problems.append(f"{name}'s floor is not a fraction in (0, 1]")
    return problems


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


def fixtures_hold(section: dict, name: str, draft: dict, stored: dict) -> list[str]:
    """A base or valid draft is a state the mandate rules can produce: a version record states the
    classification mandate spec §9.2 gives its two stored documents, and an admission through a
    thesis names an unpinned mandate (MI-20)."""
    p, problems = draft["payload"], []
    if draft["event_type"] == "MandateVersionApplied":
        old, new = stored.get(p["old_version"]), stored.get(p["new_version"])
        if old is not None and new is not None:
            verdict = classify(old, new)
            if verdict != p["classification"]:
                problems.append(found("drafts.classification", f"{name}: states {p['classification']}, §9.2 gives {verdict}"))
    if draft["event_type"] == "UniverseChanged" and p["reason"] == "thesis_admitted":
        mandate = stored.get(draft["config_refs"].get("mandate_version"))
        if mandate is None or mandate["universe"]["pinned"]:
            problems.append(found("drafts.thesis_under_research", f"{name}: a thesis admits under a pinned mandate"))
    return problems


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    stored = {a["ref"]: a for a in section["artifacts"]}
    objects = {ref: a["object"] for ref, a in stored.items()}
    for a in section["artifacts"]:
        if canonical_of(a["object"]) != a["canonical"] or "sha256:" + sha(a["canonical"]) != a["ref"]:
            problems.append(found("artifacts.rehash", f"artifact {a['name']}: does not re-hash"))
    base = mandate_document()
    for a in section["artifacts"]:
        for problem in document_problems(a["name"], a["object"], base):
            problems.append(found("documents.valid", f"artifact {problem}"))
    for name, draft in section["drafts"].items():
        for ref in draft["artifact_refs"]:
            if ref not in stored:
                problems.append(found("artifacts.missing", f"draft {name}: artifact {ref} missing"))
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
        problems += fixtures_hold(section, name, draft, objects)
    if recompute_facts(section) != section["journaled_facts"]:
        problems.append(found("facts.recompute", "the listed facts are not what the drafts map to"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
        problems += fixtures_hold(section, case["name"], draft, objects)
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.29",
    "rule.30",
    "rule.30.reason",
    "rule.31",
    "rule.32",
    "rule.32.thesis",
    "rule.32.pinned",
    "rule.33",
    "rule.33.allocation",
    "rule.33.floor",
    "rule.33.reason",
    "loose.payload.agent_id",
    "loose.payload.old_version",
    "loose.payload.instrument",
    "types.asset_id",
    "types.asset_id_case",
    "types.asset_id_ident",
    "types.asset_id_trailing_newline",
    "order.rule_30_first",
    "order.rule_33_first",
    "order.rule_32_first",
    "loose.payload.classification",
    "loose.payload.universe_size_after",
    "loose.payload.thesis_id",
    "loose.payload.lineage_id",
    "loose.payload.max_loss_from_allocation",
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

    def valid_case(s, name):
        return next(c for c in s["valid_drafts"] if c["name"] == name)

    def edited(s, name, path, value):
        a = artifact(s, name)
        node = a["object"]
        *parents, last = path.split("/")
        for p in parents:
            node = node[p]
        node[last] = value
        a["canonical"] = canonical_of(a["object"])
        a["ref"] = "sha256:" + sha(a["canonical"])

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
            "a stored version is not a valid mandate",
            "documents.valid",
            mutated(lambda s: edited(s, "renamed_document", "risk/max_order_usd", "999999")),
        ),
        (
            "a base draft breaks rule 29",
            "drafts.valid",
            mutated(lambda s: s["drafts"]["version_rejected"]["payload"].update(step_up=None)),
        ),
        (
            "a valid draft relabels a raise as neutral and drops step-up",
            "drafts.classification",
            mutated(
                lambda s: valid_case(s, "neutral_without_step_up")["changes"].__setitem__(
                    slice(0, 3), versions(refs()["old_document"], refs()["tightened_document"])
                )
            ),
        ),
        (
            "a thesis admits under a pinned mandate",
            "drafts.thesis_under_research",
            mutated(lambda s: s["drafts"]["universe_admitted"]["config_refs"].update(mandate_version=refs()["old_document"])),
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
            mutated(lambda s: valid_case(s, "pinned_instrument_admitted")["changes"].append(change("payload.result", "x"))),
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

