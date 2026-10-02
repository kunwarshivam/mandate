"""Journal spec v0.9 §9.4's reference vectors (DEC-413): the agent stream's `ThesisProposed` and
`ThesisRevised`.

The schemas and rules 34 to 38 live in `control.py`, beside §9.2's and §9.3's, so one validator
judges every closed schema. This module builds the `research` section: the stored artifacts the
records name (the research agent's mandate, its model content, and each thesis's prompt, response,
evidence, and autopsy), base drafts of both records on a research agent's stream, an invalid draft
for every rule and member type, and valid drafts for the cases a rule might be misread to refuse. It
checks the section with its own oracles: the stored mandate is the proven research base with only
its admitting model's content hash set to the stored model content, every base and valid draft names
that pinned model, and every base and valid draft's verdict agrees with the §8.5 checks the mandate
alone decides. Every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
import functools

import yaml

from common import artifact_ref, change, delete, digest_strings, is_ulid
from control import (
    CHECK_NUMBER,
    MANDATE_CASES,
    THESIS_ORDER_BUGS,
    WORKSPACE,
    canonical_of,
    check_of,
    draft_for,
    reported,
    sha,
    violations,
)
from control import (
    invalid as control_invalid,
)
from control import (
    valid as control_valid,
)
from risk_state import ADMITTED_ASSET, proven_research_document

SPEC = "docs/specs/journal.md v0.9 §9.4 (DEC-413)"
RESEARCHER = "agent_r"
STREAM = f"agent:{WORKSPACE}:{RESEARCHER}"
RUNTIME = {"kind": "agent", "id": RESEARCHER, "version": "0.1.0", "build": "sha256:" + "c" * 64}
REFUSED_ASSET = "7b4a1c2e-3333-4a2b-9c3d-000000000003"
ADMITTING_MODEL = "llm.research_agent"
BASE_IDS = {
    "proposed_admitted": "01J8ZTA0A000000000000000T1",
    "proposed_refused": "01J8ZTA1A000000000000000T2",
    "proposed_ignored": "01J8ZTA2A000000000000000T3",
    "revised_admitted": "01J8ZTB0A000000000000000T4",
    "revised_retired": "01J8ZTB1A000000000000000T5",
}
assert all(is_ulid(i) for i in BASE_IDS.values()), "event IDs (§3)"
AS_OF = "2026-09-22T14:00:00.000000000Z"
HORIZON_S = 432000
EXPIRES_AT = "2026-09-27T14:00:00.000000000Z"
SOURCES = ["filings_sec_edgar", "news_wire_a"]
LINEAGE = "th_01J8ZTA0"

MODEL_CONTENT = {
    "kind": "signal_model",
    "model_id": ADMITTING_MODEL,
    "model_version": "0.1.0",
    "authorship": "platform",
    "code": "reference fixture: stands for the research agent's prompt, code, and model identity (mandate spec §8.1, §8.4)",
}


def text_artifact(kind: str, thesis: str) -> dict:
    """A stand-in for a prompt, response, evidence bundle, or autopsy: the vectors need only stored
    objects that re-hash, never model text."""
    return {"kind": kind, "thesis_id": thesis, "note": f"reference fixture: stands for the {kind} of {thesis}"}


THESES = ("th_01J8ZTA0", "th_01J8ZTA1", "th_01J8ZTA2", "th_01J8ZTB0", "th_01J8ZTB1")


def research_mandate() -> dict:
    """The mandate reference cases' `research_equity` with its admitting model's content hash set to
    the stored model content, so the record's `content_hash` names an object that re-hashes."""
    document = copy.deepcopy(proven_research_document())
    for model in document["behavior"]["signal_models"]:
        if model["admits_instruments"]:
            model["content_hash"] = artifact_ref(MODEL_CONTENT)
    return document


def artifacts() -> dict[str, dict]:
    out = {"research_mandate": research_mandate(), "model_content": MODEL_CONTENT}
    for thesis in THESES:
        for kind in ("prompt", "response", "evidence"):
            out[f"{kind}_{thesis}"] = text_artifact(kind, thesis)
    for thesis in ("th_01J8ZTB0", "th_01J8ZTB1"):
        out[f"autopsy_{thesis}"] = text_artifact("autopsy", thesis)
    return out


def ref(name: str) -> str:
    return artifact_ref(artifacts()[name])


def thesis_payload(thesis: str, **members) -> dict:
    payload = {
        "model_id": ADMITTING_MODEL,
        "model_version": "0.1.0",
        "content_hash": ref("model_content"),
        "thesis_id": thesis,
        "lineage_id": thesis,
        "revision": 0,
        "predecessor_thesis_id": None,
        "autopsy_ref": None,
        "instrument_id": ADMITTED_ASSET,
        "asset_class": "us_equity",
        "direction": "long",
        "as_of": AS_OF,
        "expires_at": EXPIRES_AT,
        "horizon_s": HORIZON_S,
        "conviction": "0.62",
        "confidence": "0.7",
        "evidence_ref": ref(f"evidence_{thesis}"),
        "evidence_sources": list(SOURCES),
        "corroboration": "market_data",
        "invalidation": "Guidance withdrawn, or a close below the prior quarter's low",
        "allowlist_version": 7,
        "prompt_ref": ref(f"prompt_{thesis}"),
        "response_ref": ref(f"response_{thesis}"),
        "admitted": True,
        "reason": None,
    }
    payload.update(members)
    return payload


def base_drafts() -> dict[str, dict]:
    mandate = ref("research_mandate")

    def draft(name, event_type, at, payload):
        return {
            "envelope_version": 1,
            "environment": "paper",
            "event_id": BASE_IDS[name],
            "stream_id": STREAM,
            "event_type": event_type,
            "schema_version": 1,
            "event_time": at,
            "clock_source": "local",
            "causation_id": None,
            "correlation_id": None,
            "actor": dict(RUNTIME),
            "config_refs": {"mandate_version": mandate, "model_version": payload["content_hash"]},
            "payload": payload,
            "artifact_refs": sorted(digest_strings(payload)),
            "pii_refs": [],
        }

    first, refused, ignored, revision, retired = THESES
    return {
        "proposed_admitted": draft(
            "proposed_admitted", "ThesisProposed", "2026-09-22T14:00:05.000000000Z", thesis_payload(first)
        ),
        "proposed_refused": draft(
            "proposed_refused",
            "ThesisProposed",
            "2026-09-22T14:10:05.000000000Z",
            thesis_payload(
                refused,
                instrument_id=REFUSED_ASSET,
                corroboration="independent_source",
                admitted=False,
                reason="eligibility_floor",
            ),
        ),
        "proposed_ignored": draft(
            "proposed_ignored",
            "ThesisProposed",
            "2026-09-22T14:20:05.000000000Z",
            thesis_payload(ignored, direction="short", corroboration=None, admitted=False, reason="direction_not_allowed"),
        ),
        "revised_admitted": draft(
            "revised_admitted",
            "ThesisRevised",
            "2026-09-28T14:00:05.000000000Z",
            thesis_payload(
                revision,
                lineage_id=LINEAGE,
                revision=1,
                predecessor_thesis_id=first,
                autopsy_ref=ref(f"autopsy_{revision}"),
                as_of="2026-09-28T14:00:00.000000000Z",
                expires_at="2026-10-03T14:00:00.000000000Z",
                corroboration="independent_source",
            ),
        ),
        "revised_retired": draft(
            "revised_retired",
            "ThesisRevised",
            "2026-10-04T14:00:05.000000000Z",
            thesis_payload(
                retired,
                lineage_id=LINEAGE,
                revision=4,
                # Revision 4's predecessor is revision 3, a thesis no draft here records.
                predecessor_thesis_id="th_01J8ZTB0D",
                autopsy_ref=ref(f"autopsy_{retired}"),
                as_of="2026-10-04T14:00:00.000000000Z",
                expires_at="2026-10-09T14:00:00.000000000Z",
                admitted=False,
                reason="lineage_retired",
            ),
        ),
    }


def retagged(base: str, changes: list[dict]) -> list[dict]:
    """The changes, then `artifact_refs` set to the digests the changed payload names, so a draft
    breaks only the rule it is built to break."""
    draft = copy.deepcopy(base_drafts()[base])
    section = {"drafts": {base: draft}}
    changed = draft_for(section, {"base_draft": base, "changes": changes})
    refs = sorted(digest_strings(changed["payload"]))
    return changes if refs == draft["artifact_refs"] else [*changes, change("artifact_refs", refs)]


def invalid(name, clause, base, changes, reason, path, also=()):
    return control_invalid(name, clause, base, retagged(base, changes), reason, path, also)


def valid(name, clause, base, changes):
    return control_valid(name, clause, base, retagged(base, changes))


# One ill-typed value per member, each refused by that member's type alone: the base draft it is
# applied to, the value, and the reason the type gives (§9.1's table).
MEMBER_TYPES = (
    ("model_id", "proposed_admitted", "", "non_canonical"),
    ("model_version", "proposed_admitted", 1, "schema"),
    ("content_hash", "proposed_admitted", "4" * 64, "non_canonical"),
    ("thesis_id", "proposed_admitted", "thesis 01", "non_canonical"),
    ("lineage_id", "revised_admitted", "lineage/01", "non_canonical"),
    ("revision", "revised_admitted", "1", "schema"),
    ("predecessor_thesis_id", "revised_admitted", "th 01", "non_canonical"),
    ("autopsy_ref", "revised_admitted", "autopsy.md", "non_canonical"),
    ("instrument_id", "proposed_admitted", "BRK.B", "non_canonical"),
    ("asset_class", "proposed_admitted", "us_option", "non_canonical"),
    ("direction", "proposed_admitted", "", "non_canonical"),
    ("as_of", "proposed_admitted", "2026-09-22T14:00:00Z", "non_canonical"),
    ("expires_at", "proposed_admitted", 1790517600, "schema"),
    ("horizon_s", "proposed_admitted", "432000", "schema"),
    ("conviction", "proposed_admitted", "62%", "non_canonical"),
    ("confidence", "proposed_admitted", 1, "schema"),
    ("evidence_ref", "proposed_admitted", "sha256:EVIDENCE", "non_canonical"),
    ("evidence_sources", "proposed_admitted", "news_wire_a", "schema"),
    ("corroboration", "proposed_admitted", "model_asserted", "non_canonical"),
    ("invalidation", "proposed_admitted", "", "non_canonical"),
    ("allowlist_version", "proposed_admitted", "7", "schema"),
    ("prompt_ref", "proposed_admitted", None, "schema"),
    ("response_ref", "proposed_admitted", "4" * 64, "non_canonical"),
    ("admitted", "proposed_admitted", "true", "schema"),
    ("reason", "proposed_refused", "stale_thesis", "non_canonical"),
)


FOREIGN_MODEL_REF = "sha256:" + "2" * 64
AS_A_REVISION = [change("payload.revision", 1), change("payload.predecessor_thesis_id", "th_01J8ZT00")]


def order_drafts() -> list[dict]:
    """One draft for each pair of §9.4's report-order groups, and one breaking all four: rule 34
    (`payload.revision`), its autopsy clause (`payload.autopsy_ref`), rules 35 to 37
    (`payload.reason`), and rule 38 (`payload.content_hash`). Each expects every violation in
    number order, so each `order.*` seeded bug that swaps two groups is caught by that pair's draft."""
    groups = {
        "revision": (AS_A_REVISION, ("schema", "payload.revision")),
        "autopsy": ([change("payload.autopsy_ref", ref(f"autopsy_{THESES[3]}"))], ("schema", "payload.autopsy_ref")),
        "reason": ([change("payload.reason", "universe_full")], ("schema", "payload.reason")),
        "model": ([change("config_refs.model_version", FOREIGN_MODEL_REF)], ("schema", "payload.content_hash")),
    }
    names = list(groups)
    pairs = [(a, b) for i, a in enumerate(names) for b in names[i + 1 :]]
    out = []
    for chosen in [*pairs, tuple(names)]:
        changes = copy.deepcopy([c for name in chosen for c in groups[name][0]])
        (reason, path), *rest = [groups[name][1] for name in chosen]
        title = "_and_".join(chosen)
        out.append(
            invalid(
                f"order_{title}",
                f"report order: {', '.join(chosen)}, in number order",
                "proposed_admitted",
                changes,
                reason,
                path,
                also=rest,
            )
        )
    return out


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule, or the rules its `also` lists; together they cover every
    §9.4 member type and rule."""
    first = THESES[0]
    typed = [
        invalid(f"{member}_ill_typed", "§9.1 types", base, [change(f"payload.{member}", value)], reason, f"payload.{member}")
        for member, base, value, reason in MEMBER_TYPES
    ]
    return [
        *typed,
        invalid(
            "a_source_cited_as_empty_text",
            "§9.1 text, inside the list",
            "proposed_admitted",
            [change("payload.evidence_sources", ["filings_sec_edgar", ""])],
            "non_canonical",
            "payload.evidence_sources[1]",
        ),
        invalid(
            "proposed_carries_a_score",
            "§9.4 closed schema",
            "proposed_admitted",
            [change("payload.score", "0.4")],
            "schema",
            "payload.score",
        ),
        invalid(
            "proposed_without_allowlist_version",
            "§9.1 absent member",
            "proposed_admitted",
            [delete("payload.allowlist_version")],
            "schema",
            "payload.allowlist_version",
        ),
        invalid(
            "proposed_reason_absent_not_null",
            "§9.1 absent member: never read as null",
            "proposed_admitted",
            [delete("payload.reason")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "proposed_as_a_revision",
            "rule 34: a ThesisProposed is revision 0",
            "proposed_admitted",
            copy.deepcopy(AS_A_REVISION),
            "schema",
            "payload.revision",
        ),
        invalid(
            "revised_at_revision_zero",
            "rule 34: a ThesisRevised is a revision above 0",
            "revised_admitted",
            [change("payload.revision", 0), change("payload.predecessor_thesis_id", None)],
            "schema",
            "payload.revision",
        ),
        invalid(
            "proposed_with_an_autopsy",
            "rule 34: only a revision carries an autopsy",
            "proposed_admitted",
            [change("payload.autopsy_ref", ref(f"autopsy_{THESES[3]}"))],
            "schema",
            "payload.autopsy_ref",
        ),
        invalid(
            "admitted_with_a_reason",
            "rule 35",
            "proposed_admitted",
            [change("payload.reason", "universe_full")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "refused_without_a_reason",
            "rule 35",
            "proposed_refused",
            [change("payload.reason", None)],
            "schema",
            "payload.reason",
        ),
        invalid(
            "short_admitted",
            "rule 36: check 1 fails on the record",
            "proposed_admitted",
            [change("payload.direction", "short")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "horizon_off_by_a_nanosecond_admitted",
            "rule 36: check 2 compares the instants exactly",
            "proposed_admitted",
            [change("payload.expires_at", "2026-09-27T14:00:00.000000001Z")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "revision_without_predecessor_admitted",
            "rule 36: check 3 fails on the record",
            "revised_admitted",
            [change("payload.predecessor_thesis_id", None)],
            "schema",
            "payload.reason",
        ),
        invalid(
            "short_and_off_horizon_refused_for_the_horizon",
            "rule 36: the first failing check decides",
            "proposed_ignored",
            [change("payload.expires_at", "2026-09-27T14:00:01.000000000Z"), change("payload.reason", "horizon_mismatch")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "short_and_without_predecessor_refused_for_the_predecessor",
            "rule 36: checks 1 and 3 both fail, and check 1 decides",
            "revised_admitted",
            [change("payload.direction", "short"), change("payload.predecessor_thesis_id", None),
             change("payload.admitted", False), change("payload.reason", "revision_without_predecessor")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "off_horizon_and_without_predecessor_refused_for_the_predecessor",
            "rule 36: checks 2 and 3 both fail, and check 2 decides",
            "revised_admitted",
            [change("payload.expires_at", "2026-10-03T14:00:01.000000000Z"), change("payload.predecessor_thesis_id", None),
             change("payload.admitted", False), change("payload.reason", "revision_without_predecessor")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "long_refused_as_short",
            "rule 36: a check the record passes is never the reason",
            "proposed_refused",
            [change("payload.reason", "direction_not_allowed")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "on_horizon_refused_as_a_mismatch",
            "rule 36: a check the record passes is never the reason",
            "proposed_refused",
            [change("payload.reason", "horizon_mismatch")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "uncorroborated_admitted",
            "rule 37: check 15 fails on the record",
            "proposed_admitted",
            [change("payload.corroboration", None)],
            "schema",
            "payload.reason",
        ),
        invalid(
            "uncorroborated_refused_by_a_later_check",
            "rule 37: an uncorroborated thesis never reaches check 16",
            "revised_retired",
            [change("payload.corroboration", None)],
            "schema",
            "payload.reason",
        ),
        invalid(
            "uncorroborated_refused_as_full",
            "rule 37: an uncorroborated thesis never reaches check 17",
            "proposed_refused",
            [change("payload.corroboration", None), change("payload.reason", "universe_full")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "corroborated_refused_as_uncorroborated",
            "rule 37: a corroborated thesis passes check 15",
            "proposed_refused",
            [change("payload.reason", "no_corroboration")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "model_ref_names_another_model",
            "rule 38",
            "proposed_admitted",
            [change("config_refs.model_version", FOREIGN_MODEL_REF)],
            "schema",
            "payload.content_hash",
        ),
        invalid(
            "admitted_and_short_and_uncorroborated",
            "rules 36 and 37: each reported once, in number order",
            "proposed_admitted",
            [change("payload.direction", "short"), change("payload.corroboration", None)],
            "schema",
            "payload.reason",
            also=[("schema", "payload.reason")],
        ),
        *order_drafts(),
        invalid(
            "proposed_without_model_ref",
            "§9 required config_refs: mod",
            "proposed_admitted",
            [change("config_refs", {"mandate_version": ref("research_mandate")})],
            "missing_config_ref",
            "config_refs.model_version",
        ),
        invalid(
            "revised_without_mandate_ref",
            "§9 required config_refs: man",
            "revised_admitted",
            [change("config_refs", {"model_version": ref("model_content")})],
            "missing_config_ref",
            "config_refs.mandate_version",
        ),
        invalid(
            "prompt_not_listed",
            "§3 artifact_refs",
            "proposed_admitted",
            [change("artifact_refs", sorted(set(digest_strings(base_drafts()["proposed_admitted"]["payload"])) - {ref(f"prompt_{first}")}))],
            "artifact_refs",
            "artifact_refs",
        ),
        invalid(
            "proposed_on_the_account_stream",
            "§9 stream",
            "proposed_admitted",
            [change("stream_id", f"acct:{WORKSPACE}:01J8Z2ACCT00000000000000A1")],
            "wrong_stream",
            "event_type",
        ),
        invalid(
            "revised_on_the_control_stream",
            "§9 stream",
            "revised_admitted",
            [change("stream_id", f"ctl:{WORKSPACE}")],
            "wrong_stream",
            "event_type",
        ),
    ]


def valid_drafts() -> list[dict]:
    """Each is a record the §8.5 checks and a correct writer can produce, so no rule refuses it."""
    return [
        valid(
            "first_thesis_with_a_predecessor_is_recorded_ignored",
            "rules 34 and 36: check 3 refuses it, and the record keeps it",
            "proposed_refused",
            [change("payload.predecessor_thesis_id", "th_01J8ZT00"), change("payload.reason", "revision_without_predecessor")],
        ),
        valid(
            "revision_without_predecessor_is_recorded_ignored",
            "rules 34 and 36: the entry type follows the revision, not the verdict",
            "revised_admitted",
            [change("payload.predecessor_thesis_id", None), change("payload.admitted", False),
             change("payload.reason", "revision_without_predecessor")],
        ),
        valid(
            "revision_without_an_autopsy",
            "rule 34: no rule requires the autopsy (DEC-413 item 5)",
            "revised_admitted",
            [change("payload.autopsy_ref", None)],
        ),
        valid(
            "short_and_off_horizon_refused_for_the_direction",
            "rule 36: the first failing check decides",
            "proposed_ignored",
            [change("payload.expires_at", "2026-09-27T14:00:01.000000000Z")],
        ),
        valid(
            "uncorroborated_refused_as_uncorroborated",
            "rule 37",
            "proposed_refused",
            [change("payload.corroboration", None), change("payload.reason", "no_corroboration")],
        ),
        valid(
            "uncorroborated_refused_by_an_earlier_check",
            "rule 37: an earlier check's refusal stands",
            "proposed_refused",
            [change("payload.corroboration", None)],
        ),
        valid(
            "admitted_citing_no_source_on_market_data",
            "§9.4 evidence_sources: an empty list, and market data corroborates",
            "proposed_admitted",
            [change("payload.evidence_sources", []), change("payload.evidence_ref", None)],
        ),
        valid(
            "mc_n07_sources_are_recorded_as_cited",
            "§9.4 evidence_sources: MC-N07's pair, out of order and off the allowlist, recorded as given",
            "proposed_refused",
            [change("payload.evidence_sources", ["src.filings", "src.anonymous_blog"]),
             change("payload.reason", "source_not_allowlisted")],
        ),
        valid(
            "a_source_cited_twice_is_kept",
            "§9.4 evidence_sources: duplicates kept, as the model gave them",
            "proposed_admitted",
            [change("payload.evidence_sources", [SOURCES[0], SOURCES[1], SOURCES[0]])],
        ),
        valid(
            "refused_when_the_universe_is_full",
            "rules 35 to 37: check 17 is reached only past check 15",
            "proposed_refused",
            [change("payload.reason", "universe_full")],
        ),
        valid(
            "zero_horizon_on_the_instant",
            "rule 36: a zero horizon passes check 2 when the instants agree",
            "proposed_refused",
            [change("payload.horizon_s", 0), change("payload.expires_at", AS_OF)],
        ),
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = (
    "artifacts.rehash",
    "artifacts.missing",
    "documents.valid",
    "drafts.valid",
    "drafts.pinned_model",
    "drafts.mandate_checks",
    "cases.listed",
    "drafts.reference_cases",
    "invalid_drafts",
    "valid_drafts",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def admitting_models(document: dict) -> list[dict]:
    return [m for m in document["behavior"]["signal_models"] if m["admits_instruments"]]


def document_problems(name: str, document: dict) -> list[str]:
    """The research mandate is the proven `research_equity` base with only its one admitting model's
    content hash changed, to the stored model content's; the model content names that model."""
    if name == "model_content":
        return [] if document == MODEL_CONTENT else [f"{name} is not the research agent's content"]
    if name != "research_mandate":
        return []
    base = proven_research_document()
    expected = copy.deepcopy(base)
    models = admitting_models(expected)
    if len(models) != 1:
        return [f"{name}'s base does not have exactly one admitting model"]
    models[0]["content_hash"] = artifact_ref(MODEL_CONTENT)
    if document != expected:
        return [f"{name} differs from the proven research base at more than its admitting model's content hash"]
    if models[0]["id"] != MODEL_CONTENT["model_id"] or models[0]["version"] != MODEL_CONTENT["model_version"]:
        return [f"{name}'s admitting model is not the stored model content's"]
    return []


# The §8.5 checks the stored mandate decides, by number. The policy overlay only tightens (mandate
# spec §4.3), so a check the mandate fails always fails, and the record's verdict never passes over
# it. Checks 5, 6, and 10 read the mandate alone, so a refusal at any of them must fail by the mandate.
# Check 6 belongs here only while the overlay's `effective_admission` raises `auto` to `ask` and
# never to `deny` (`PolicyOverlay::effective_admission`); an overlay that could deny moves it out.
# The overlay can fail check 4 (`research_agent_allowed`) and check 16 has the folded `retired` flag
# besides the cap, so a refusal at either need not fail by the mandate.
def mandate_failures(document: dict, payload: dict) -> list[int]:
    research = document["behavior"]["research"]
    universe = document["universe"]
    failures = []
    if research is None or not admitting_models(document):
        failures.append(4)
    if universe["pinned"]:
        failures.append(5)
    if document["autonomy"]["admission"] == "deny":
        failures.append(6)
    if payload["asset_class"] not in universe["asset_classes"]:
        failures.append(10)
    cap = 0 if research is None else research["max_revisions_per_lineage"]
    if payload["revision"] > cap:
        failures.append(16)
    return failures


DECIDED_BY_THE_MANDATE = (5, 6, 10)


def fixtures_hold(name: str, draft: dict, stored: dict) -> list[str]:
    """A base or valid draft is a record the research agent's own mandate can produce."""
    p, problems = draft["payload"], []
    document = stored.get(draft["config_refs"].get("mandate_version"))
    if document is None:
        return [found("drafts.pinned_model", f"{name}: its mandate is not stored")]
    pinned = [(m["id"], m["version"], m["content_hash"]) for m in admitting_models(document)]
    if (p["model_id"], p["model_version"], p["content_hash"]) not in pinned:
        problems.append(found("drafts.pinned_model", f"{name}: names a model its mandate does not pin to admit"))
    failures = mandate_failures(document, p)
    number = CHECK_NUMBER.get(p["reason"]) if p["reason"] is not None else None
    if number in DECIDED_BY_THE_MANDATE and number not in failures:
        problems.append(found("drafts.mandate_checks", f"{name}: refused at check {number}, which its mandate passes"))
    if failures and (number is None or number > failures[0]):
        problems.append(found("drafts.mandate_checks", f"{name}: its mandate fails check {failures[0]} first"))
    return problems


THESIS_EVENTS = ("ThesisProposed", "ThesisRevised")
PLACEHOLDER_PROMPT = "sha256:" + "a" * 64
PLACEHOLDER_RESPONSE = "sha256:" + "b" * 64


@functools.cache
def mandate_cases() -> dict:
    return yaml.safe_load(MANDATE_CASES.read_text(encoding="utf-8"))


def journaled_theses(value):
    """Every `ThesisProposed` and `ThesisRevised` entry in a case's expectation, in file order."""
    if isinstance(value, dict):
        if value.get("type") in THESIS_EVENTS:
            yield value
        for inner in value.values():
            yield from journaled_theses(inner)
    elif isinstance(value, list):
        for inner in value:
            yield from journaled_theses(inner)


def reference_case_listing() -> list[dict]:
    """The thesis records the approved mandate reference cases journal: case, thesis, and type."""
    return [
        {"case": case["id"], "thesis_id": entry["thesis_id"], "event_type": entry["type"]}
        for case in mandate_cases()["cases"]
        for entry in journaled_theses(case.get("expect"))
    ]


def reference_case_draft(listed: dict) -> dict:
    """The §9.4 record a correct writer makes for one listed entry: the case's thesis input, the
    entry's verdict and platform-derived corroboration, and the case base's admitting model. Members
    no case states (the artifacts, the allowlist version) take placeholders, which no rule reads."""
    cases = mandate_cases()
    case = next(c for c in cases["cases"] if c["id"] == listed["case"])
    entry = next(e for e in journaled_theses(case["expect"]) if e["thesis_id"] == listed["thesis_id"])
    inputs = case["input"]
    theses = inputs["theses"] if "theses" in inputs else [inputs["thesis"]]
    thesis = next(t for t in theses if t["thesis_id"] == listed["thesis_id"])
    base = cases["bases"][case["base"]]
    models = base["mandate"]["behavior"]["signal_models"]
    # A base whose research agent cannot admit (a pinned universe, MI-20) still names it as `llm.`.
    (model,) = admitting_models(base["mandate"]) or [m for m in models if m["id"].startswith("llm.")]
    stated = thesis["corroboration"]["kind"] if thesis.get("corroboration") else None
    payload = {
        "model_id": model["id"],
        "model_version": model["version"],
        "content_hash": model["content_hash"],
        "thesis_id": thesis["thesis_id"],
        "lineage_id": thesis["lineage_id"],
        "revision": thesis["revision"],
        "predecessor_thesis_id": thesis["predecessor_thesis_id"],
        "autopsy_ref": None,
        "instrument_id": thesis["instrument_id"],
        "asset_class": thesis["asset_class"],
        "direction": thesis["direction"],
        "as_of": thesis["as_of"],
        "expires_at": thesis["expires_at"],
        "horizon_s": thesis["horizon_s"],
        "conviction": thesis["conviction"],
        "confidence": thesis["confidence"],
        "evidence_ref": None,
        "evidence_sources": list(thesis["evidence_sources"]),
        "corroboration": entry.get("corroboration", stated),
        "invalidation": thesis["invalidation"],
        "allowlist_version": 1,
        "prompt_ref": PLACEHOLDER_PROMPT,
        "response_ref": PLACEHOLDER_RESPONSE,
        "admitted": entry["admitted"],
        "reason": entry.get("reason"),
    }
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": BASE_IDS["proposed_admitted"],
        "stream_id": STREAM,
        "event_type": listed["event_type"],
        "schema_version": 1,
        "event_time": thesis["as_of"],
        "clock_source": "local",
        "causation_id": None,
        "correlation_id": None,
        "actor": dict(RUNTIME),
        "config_refs": {"mandate_version": base["canonical_sha256"], "model_version": model["content_hash"]},
        "payload": payload,
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
    }


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    stored = {a["ref"]: a for a in section["artifacts"]}
    objects = {r: a["object"] for r, a in stored.items()}
    for a in section["artifacts"]:
        if canonical_of(a["object"]) != a["canonical"] or "sha256:" + sha(a["canonical"]) != a["ref"]:
            problems.append(found("artifacts.rehash", f"artifact {a['name']}: does not re-hash"))
        for problem in document_problems(a["name"], a["object"]):
            problems.append(found("documents.valid", f"artifact {problem}"))
    for name, draft in section["drafts"].items():
        for r in draft["artifact_refs"]:
            if r not in stored:
                problems.append(found("artifacts.missing", f"draft {name}: artifact {r} missing"))
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
        problems += fixtures_hold(name, draft, objects)
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        draft = draft_for(section, case)
        for r in draft["artifact_refs"]:
            if r not in stored:
                problems.append(found("artifacts.missing", f"valid draft {case['name']}: artifact {r} missing"))
        got = violations(draft)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
        problems += fixtures_hold(case["name"], draft, objects)
    if section["reference_cases"] != reference_case_listing():
        problems.append(found("cases.listed", "the listing is not every thesis record the mandate cases journal"))
    for listed in section["reference_cases"]:
        got = violations(reference_case_draft(listed))
        if got:
            problems.append(found("drafts.reference_cases", f"{listed['case']} {listed['thesis_id']}: {got}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.34",
    "rule.34.autopsy",
    "rule.35",
    "rule.36",
    "rule.36.direction",
    "rule.36.horizon",
    "rule.36.predecessor",
    "rule.36.unfailed",
    "rule.36.order",
    "rule.37",
    "rule.37.corroborated",
    "rule.38",
    *THESIS_ORDER_BUGS,
    "config_refs.required",
    "open.payload",
    "record.missing",
    *(f"loose.payload.{member}" for member, _, _, _ in MEMBER_TYPES),
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

    def edited(s, name, fn):
        a = artifact(s, name)
        fn(a["object"])
        a["canonical"] = canonical_of(a["object"])
        a["ref"] = "sha256:" + sha(a["canonical"])

    return [
        (
            "an artifact's object edited",
            "artifacts.rehash",
            mutated(lambda s: artifact(s, f"prompt_{THESES[0]}")["object"].update(note="edited")),
        ),
        (
            "a base draft names an artifact not stored",
            "artifacts.missing",
            mutated(lambda s: s["drafts"]["proposed_refused"]["artifact_refs"].append("sha256:" + "e" * 64)),
        ),
        (
            "the stored mandate is not the proven research base",
            "documents.valid",
            mutated(lambda s: artifact(s, "research_mandate")["object"]["universe"].update(max_instruments=9)),
        ),
        (
            "a base draft breaks rule 35",
            "drafts.valid",
            mutated(lambda s: s["drafts"]["proposed_admitted"]["payload"].update(reason="universe_full")),
        ),
        (
            "a valid draft names a model its mandate does not pin",
            "drafts.pinned_model",
            mutated(lambda s: valid_case(s, "revision_without_an_autopsy")["changes"].append(change("payload.model_version", "0.2.0"))),
        ),
        (
            "a valid draft admits an asset class its mandate does not allow",
            "drafts.mandate_checks",
            mutated(lambda s: valid_case(s, "revision_without_an_autopsy")["changes"].append(change("payload.asset_class", "crypto"))),
        ),
        (
            "a valid draft is refused at a check its mandate alone passes",
            "drafts.mandate_checks",
            mutated(
                lambda s: valid_case(s, "uncorroborated_refused_by_an_earlier_check")["changes"].append(
                    change("payload.reason", "universe_pinned")
                )
            ),
        ),
        (
            "a valid draft is refused at admission_denied under a mandate that asks",
            "drafts.mandate_checks",
            mutated(
                lambda s: valid_case(s, "uncorroborated_refused_by_an_earlier_check")["changes"].append(
                    change("payload.reason", "admission_denied")
                )
            ),
        ),
        (
            "a mandate case's thesis record is left out of the listing",
            "cases.listed",
            mutated(lambda s: s["reference_cases"].pop()),
        ),
        (
            "a mandate case's first thesis is listed as a revision",
            "drafts.reference_cases",
            mutated(lambda s: s["reference_cases"][0].update(event_type="ThesisRevised")),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: s["invalid_drafts"][0]["expect"].update(path="payload.model_name")),
        ),
        (
            "a valid draft breaks a rule",
            "valid_drafts",
            mutated(lambda s: valid_case(s, "zero_horizon_on_the_instant")["changes"].append(change("payload.horizon_s", 1))),
        ),
        (
            "the stored mandate pins its universe and the edit re-hashes",
            "documents.valid",
            mutated(lambda s: edited(s, "research_mandate", lambda d: d["universe"].update(pinned=True))),
        ),
    ]


# Seeded bugs that add a rule rather than drop one. Each is a tightening the §9.4 text declined
# because an approved mandate case journals a record it would refuse, so each must make
# `drafts.reference_cases` fail: `tighten.sorted_sources` is #490 round 1 M1's rule, which refused
# MC-N07.
TIGHTEN_MUTANTS = ("tighten.sorted_sources",)


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught; a vector mutant only by the check it is registered against, with no
    other check of that check's family also catching it; and every tightening mutant refusing at
    least one approved mandate case's thesis record."""
    escaped = []
    reference_drafts = [reference_case_draft(listed) for listed in section["reference_cases"]]
    for mutant in TIGHTEN_MUTANTS:
        skip = frozenset([mutant])
        if not any(violations(draft, skip) for draft in reference_drafts):
            escaped.append(f"research tightening mutant {mutant}: no mandate case's thesis record is refused")
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
            escaped.append(f"research validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"research check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        family = check.partition(".")[0]
        masked = sorted(c for c in caught_by if c != check and c.partition(".")[0] == family)
        if check not in caught_by:
            escaped.append(f"research vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
        elif masked:
            escaped.append(f"research vector mutant: {name} (caught by {check} and also by {masked})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "stream_id": STREAM,
        "artifacts": [
            {"name": name, "ref": artifact_ref(obj), "object": obj, "canonical": canonical_of(obj)}
            for name, obj in artifacts().items()
        ],
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "reference_cases": reference_case_listing(),
    }
