"""Journal spec v0.49 §9.22's reference vectors for the invalidation verdict (DEC-2202, backlog
E19-5): the agent stream's `ThesisInvalidated`, the record a removal names as its cause (mandate
spec §8.6, agent harness spec §6.7).

The schema and the per-draft rules 184 and 185 live in `control.py`, beside the thesis records, so
one validator judges every closed schema. This module builds the `thesis_invalidation` section:

1. base drafts: an invalidation the model's verdict caused, one a revoked source caused, the
   research call whose judged verdict the first names, and the thesis records the stream admitted;
2. an invalid draft for every member type and rules 184 and 185, and valid drafts for the cases a
   rule might be misread to refuse;
3. batches for rule 186, the batch rule that ties a model's invalidation to its call's judged
   verdict, and every judged invalidation verdict to its record;
4. agent streams for rule 187, the stream rule that names an admitted, current, not yet invalidated
   thesis, each stream a list of `append` batches.

Every stream and batch draft also passes every other rule (rules 151 and 153 among them), and every
seeded bug, in the validator, the batch and stream rules, and the vectors, is shown caught by the
check it is registered against.
"""

from __future__ import annotations

import copy

import gateway
import research
from common import apply_change, change, delete, digest_strings
from control import AGENT_STREAM, RUNTIME, SERVICES, check_of, reported, violations

SPEC = "docs/specs/journal.md v0.49 §9.22 (DEC-2202)"
STREAM = AGENT_STREAM
MANDATE = research.ref("research_mandate")
FIRST = research.THESES[0]
REFUSED_THESIS = research.THESES[1]
REVISION = research.THESES[3]
RENEWAL = "th_01J8ZTC0"
ASSET = research.thesis_payload(FIRST)["instrument_id"]
OTHER_ASSET = research.REFUSED_ASSET
AT = "2026-09-23T14:00:41.250000000Z"
RUN = "01J9GT00A000000000000000R1"
CALL_EVENT = "01J9GX10A000000000000000V1"
CALL_ID = "01J9GX00A000000000000000C1"
INVALIDATED = "01J9GX20A000000000000000V2"
REVOKED = "01J9GX21A000000000000000V3"
ALLOWLIST = "01J9GX00B000000000000000A1"
REVOKED_SOURCE = research.SOURCES[1]
EVIDENCE = ["item_filing_10q_q3", "item_bars_2026_09_22"]
BASE_CALL = "call"
BASE_INVALIDATED = "invalidated"
BASE_REVOKED = "revoked"
BASE_ADMITTED = "admitted"
BASE_REVISED = "revised"
BASE_REFUSED = "refused"


def on_this_stream(draft: dict) -> dict:
    """A research base draft, rewritten onto this section's agent stream and its runtime."""
    out = copy.deepcopy(draft)
    out["stream_id"] = STREAM
    out["actor"] = dict(RUNTIME)
    return out


def invalidation_payload(**members) -> dict:
    payload = {
        "thesis_id": FIRST,
        "lineage_id": FIRST,
        "instrument_id": ASSET,
        "cause": "model",
        "candidate": 0,
        "evidence": list(EVIDENCE),
        "source": None,
    }
    payload.update(members)
    return payload


def invalidation_draft(event_id: str, causation: str | None, payload: dict) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id,
        "stream_id": STREAM,
        "event_type": "ThesisInvalidated",
        "schema_version": 1,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": causation,
        "correlation_id": RUN,
        "actor": dict(RUNTIME),
        "config_refs": {"mandate_version": MANDATE},
        "payload": payload,
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
    }


def call_draft(event_id: str = CALL_EVENT, call_id: str = CALL_ID, verdicts: list[dict] | None = None) -> dict:
    """The research call whose response listed an invalidation the harness judged (rule 151)."""
    draft = copy.deepcopy(gateway.DRAFTS[gateway.BASE_CALL])
    draft["event_id"] = event_id
    draft["stream_id"] = STREAM
    draft["actor"] = dict(RUNTIME)
    draft["payload"]["call_id"] = call_id
    draft["payload"]["verdicts"] = verdicts if verdicts is not None else [gateway.verdict(0, "invalidation", "judged")]
    return draft


def base_drafts() -> dict[str, dict]:
    stated = research.base_drafts()
    return {
        BASE_CALL: call_draft(),
        BASE_INVALIDATED: invalidation_draft(INVALIDATED, CALL_EVENT, invalidation_payload()),
        BASE_REVOKED: invalidation_draft(
            REVOKED,
            ALLOWLIST,
            invalidation_payload(cause="source_revoked", candidate=None, evidence=[], source=REVOKED_SOURCE),
        ),
        BASE_ADMITTED: on_this_stream(stated["proposed_admitted"]),
        BASE_REVISED: on_this_stream(stated["revised_admitted"]),
        BASE_REFUSED: on_this_stream(stated["proposed_refused"]),
    }


DRAFTS = base_drafts()


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


# Each member: a value of the wrong JSON kind (`schema`) and, where a string has a form, a string of
# the wrong form (`non_canonical`).
MEMBERS = (
    ("thesis_id", 7, "th 01"),
    ("lineage_id", 7, "th 01"),
    ("instrument_id", 7, "not-an-asset-id"),
    ("cause", 7, "expired"),
    ("candidate", "0", None),
    ("evidence", "item_filing_10q_q3", None),
    ("source", 7, None),
)
NULLED = ("thesis_id", "lineage_id", "instrument_id", "cause", "evidence")


def member_drafts() -> list[dict]:
    b, clause = BASE_INVALIDATED, "§9.22 types"
    out = []
    for member, wrong_kind, wrong_form in MEMBERS:
        path = f"payload.{member}"
        out.append(invalid(f"invalidated.{member}.kind", clause, b, [change(path, wrong_kind)], "schema", path))
        if wrong_form is not None:
            out.append(invalid(f"invalidated.{member}.form", clause, b, [change(path, wrong_form)], "non_canonical", path))
    for member in NULLED:
        path = f"payload.{member}"
        out.append(invalid(f"invalidated.{member}.null", clause, b, [change(path, None)], "schema", path))
    out += [
        invalid("invalidated.evidence.item", clause, b, [change("payload.evidence", [7])], "schema", "payload.evidence[0]"),
        invalid("invalidated.missing", clause, b, [delete("payload.source")], "schema", "payload.source"),
        invalid("invalidated.extra", clause, b, [change("payload.zz_extra", True)], "schema", "payload.zz_extra"),
    ]
    return out


def rule_drafts() -> list[dict]:
    m, r = BASE_INVALIDATED, BASE_REVOKED
    return [
        invalid("invalidated.184.no_candidate", "rule 184: a model's invalidation names its verdict", m, [change("payload.candidate", None)], "schema", "payload.candidate"),
        invalid("invalidated.184.source", "rule 184: a model's invalidation names no source", m, [change("payload.source", REVOKED_SOURCE)], "schema", "payload.source"),
        invalid("invalidated.184.no_cause", "rule 184: a model's invalidation names its call", m, [change("causation_id", None)], "schema", "causation_id"),
        invalid("revoked.184.candidate", "rule 184: a revoked source's invalidation names no verdict", r, [change("payload.candidate", 0)], "schema", "payload.candidate"),
        invalid("revoked.184.evidence", "rule 184: a revoked source's invalidation cites no items", r, [change("payload.evidence", list(EVIDENCE))], "schema", "payload.evidence"),
        invalid("revoked.184.no_source", "rule 184: a revoked source's invalidation names the source", r, [change("payload.source", None)], "schema", "payload.source"),
        invalid("revoked.184.no_cause", "rule 184: a revoked source's invalidation names the allowlist that dropped it", r, [change("causation_id", None)], "schema", "causation_id"),
        invalid("invalidated.184.first", "rule 184: the first that fails, in order", m, [change("payload.candidate", None), change("payload.source", REVOKED_SOURCE)], "schema", "payload.candidate"),
        invalid("invalidated.185.actor", "rule 185", m, [change("actor", dict(SERVICES))], "schema", "actor.kind"),
        invalid("invalidated.without_mandate", "§9 row: `man`", m, [change("config_refs", {})], "missing_config_ref", "config_refs.mandate_version"),
        invalid("invalidated.on_the_control_stream", "§9: an agent-stream record", m, [change("stream_id", "ctl:ws_01J8Z2")], "wrong_stream", "event_type"),
        invalid("invalidated.at_version_2", "§9.22: schema version 1 only", m, [change("schema_version", 2)], "unknown_schema", "payload"),
    ]


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every member type and rule."""
    return [*member_drafts(), *rule_drafts()]


def valid_drafts() -> list[dict]:
    """The cases a rule might be misread to refuse; each must be accepted."""
    m = BASE_INVALIDATED
    return [
        valid("invalidated.no_evidence", "rule 184: the output may cite no item", m, [change("payload.evidence", [])]),
        valid("invalidated.evidence_as_given", "§9.22: the items as the output gave them, unordered and repeated", m, [change("payload.evidence", ["item_b", "item_a", "item_b"])]),
        valid("invalidated.later_candidate", "rule 184: any candidate index", m, [change("payload.candidate", 3)]),
        valid("invalidated.revision", "§9.22: a revision's thesis, in its lineage", m, [change("payload.thesis_id", REVISION)]),
        valid("invalidated.no_run", "§9.22: `correlation_id` is not read", m, [change("correlation_id", None)]),
    ]


def draft_for(section: dict, case: dict) -> dict:
    draft = copy.deepcopy(section["drafts"][case["base_draft"]])
    for item in case["changes"]:
        apply_change(draft, item)
    return draft


# --------------------------------------------------------------------------- rules 186 and 187


def batch_failure(drafts: list[dict], skip: frozenset[str] = frozenset()) -> dict | None:
    """Rule 186 over one `append` batch: the first draft that breaks it, with the member reported.
    A model's invalidation names an earlier call of the batch whose verdict at its `candidate` is a
    judged invalidation, which no earlier invalidation of the batch names; and every judged
    invalidation verdict of a call in the batch is named by a later invalidation of the batch."""
    named: set[tuple[str, int]] = set()
    for i, d in enumerate(drafts):
        p = d["payload"]
        if d["event_type"] == "ModelInvocationRecorded" and "batch.186.complete" not in skip:
            later = [
                (x["causation_id"], x["payload"]["candidate"])
                for x in drafts[i + 1 :]
                if x["event_type"] == "ThesisInvalidated" and x["payload"]["cause"] == "model"
            ]
            for j, v in enumerate(p["verdicts"]):
                if v["kind"] == "invalidation" and v["result"] == "judged" and (d["event_id"], j) not in later:
                    return {"index": i, "reason": "schema", "path": f"payload.verdicts[{j}].result"}
        if d["event_type"] != "ThesisInvalidated" or p["cause"] != "model":
            continue
        earlier = drafts if "batch.186.order" in skip else drafts[:i]
        call = next(
            (
                x
                for x in earlier
                if x["event_id"] == d["causation_id"]
                and (x["event_type"] == "ModelInvocationRecorded" or "batch.186.call" in skip)
            ),
            None,
        )
        if call is None:
            if "batch.186.present" in skip:
                continue
            return {"index": i, "reason": "schema", "path": "causation_id"}
        verdicts = call["payload"].get("verdicts", [])
        k = p["candidate"]
        item = verdicts[k] if 0 <= k < len(verdicts) else None
        fits = item is not None and (item["kind"] == "invalidation" or "batch.186.kind" in skip)
        fits = fits and (item["result"] == "judged" or "batch.186.result" in skip)
        if not fits:
            return {"index": i, "reason": "schema", "path": "payload.candidate"}
        if (d["causation_id"], k) in named and "batch.186.once" not in skip:
            return {"index": i, "reason": "schema", "path": "payload.candidate"}
        named.add((d["causation_id"], k))
    return None


def stream_failure(drafts: list[dict], skip: frozenset[str] = frozenset()) -> dict | None:
    """Rule 187 over an agent stream from its start: the first `ThesisInvalidated` that names a thesis
    already invalidated, or not the one the stream last admitted for its instrument in its lineage."""
    invalidated: set[str] = set()
    admitted: dict[str, dict] = {}
    holder: dict[str, str] = {}
    for i, d in enumerate(drafts):
        p = d["payload"]
        if d["event_type"] in research.THESIS_EVENTS and (p["admitted"] or "stream.187.admitted" in skip):
            admitted[p["thesis_id"]] = p
            holder[p["instrument_id"]] = p["thesis_id"]
        if d["event_type"] != "ThesisInvalidated":
            continue
        thesis = p["thesis_id"]
        fail = None
        if thesis in invalidated and "stream.187.once" not in skip:
            fail = "payload.thesis_id"
        elif thesis not in admitted:
            fail = None if "stream.187.known" in skip else "payload.thesis_id"
        elif admitted[thesis]["lineage_id"] != p["lineage_id"] and "stream.187.lineage" not in skip:
            fail = "payload.lineage_id"
        elif admitted[thesis]["instrument_id"] != p["instrument_id"] and "stream.187.instrument" not in skip:
            fail = "payload.instrument_id"
        elif holder.get(p["instrument_id"]) != thesis and "stream.187.current" not in skip:
            fail = "payload.thesis_id"
        if fail is not None:
            return {"index": i, "reason": "schema", "path": fail}
        invalidated.add(thesis)
    return None


def flat(case: dict) -> list[dict]:
    return [d for batch in case["batches"] for d in batch]


def call(n: int, verdicts: list[dict] | None = None) -> dict:
    return call_draft(f"01J9GX1{n}A000000000000000V1", f"01J9GX0{n}A000000000000000C1", verdicts)


def invalidation(n: int, of: dict | None, **members) -> dict:
    """An invalidation numbered `n`, caused by the call `of` (or the base allowlist when `None`)."""
    causation = of["event_id"] if of is not None else ALLOWLIST
    if of is None:
        members = {"cause": "source_revoked", "candidate": None, "evidence": [], "source": REVOKED_SOURCE, **members}
    return invalidation_draft(f"01J9GX2{n}A000000000000000V2", causation, invalidation_payload(**members))


def with_changes(base: str, event_id: str, changes: list[dict]) -> dict:
    draft = copy.deepcopy(DRAFTS[base])
    draft["event_id"] = event_id
    for item in changes:
        apply_change(draft, item)
    draft["artifact_refs"] = sorted(digest_strings(draft["payload"]))
    return draft


def renewal() -> dict:
    """A renewal (mandate spec §8.6): a new thesis for the same instrument in the same lineage."""
    return with_changes(BASE_ADMITTED, "01J9GX30A000000000000000T1", [change("payload.thesis_id", RENEWAL)])


def new_thesis(event_id: str = "01J9GX31A000000000000000T2") -> dict:
    """A new thesis in a new lineage for the same instrument, admitted after the old one ended."""
    return with_changes(BASE_ADMITTED, event_id, [change("payload.thesis_id", RENEWAL), change("payload.lineage_id", RENEWAL)])


def batch_cases() -> list[dict]:
    def case(name, clause, drafts, expect):
        return {"name": name, "clause": clause, "drafts": drafts, "expect": expect}

    ok = {"outcome": "Valid"}

    def at(index, path):
        return {"outcome": "Invalid", "index": index, "reason": "schema", "path": path}

    judged = gateway.verdict(0, "invalidation", "judged")
    two = [judged, gateway.verdict(1, "invalidation", "judged")]
    refused = [judged, gateway.verdict(1, "invalidation", "unknown_thesis")]
    theses = [gateway.verdict(0, "thesis", "judged"), gateway.verdict(1, "invalidation", "judged")]
    only_theses = [gateway.verdict(0, "thesis", "judged")]
    c1, c2, c3, c4, c5 = call(1), call(2, two), call(3, refused), call(4, theses), call(5, only_theses)
    return [
        case("the_call_then_its_invalidation", "rule 186: one batch, the call first", [c1, invalidation(1, c1)], ok),
        case("two_invalidations", "rule 186: each judged verdict its own record", [c2, invalidation(1, c2), invalidation(2, c2, candidate=1, thesis_id=REVISION)], ok),
        case("a_refused_invalidation_has_no_record", "rule 186: only a judged verdict is handed on", [c3, invalidation(1, c3)], ok),
        case("after_the_theses", "rule 186: a candidate counts the theses before it", [c4, invalidation(1, c4, candidate=1)], ok),
        case("a_call_with_theses_only", "rule 186: a call with no invalidation names none", [c5], ok),
        case("a_revoked_source", "rule 186: a revoked source's invalidation names no call", [invalidation(1, None)], ok),
        case("alone", "rule 186: the call is in the same batch", [invalidation(1, c1)], at(0, "causation_id")),
        case("before_its_call", "rule 186: the call is earlier in the batch", [invalidation(1, c1), c1], at(0, "causation_id")),
        case("names_another_event", "rule 186: the cause is the call, not another record of the batch", [c5, DRAFTS[BASE_ADMITTED], invalidation(1, c5) | {"causation_id": DRAFTS[BASE_ADMITTED]["event_id"]}], at(2, "causation_id")),
        case("names_a_thesis_verdict", "rule 186: the verdict is an invalidation's", [c5, invalidation(1, c5)], at(1, "payload.candidate")),
        case("names_a_refused_verdict", "rule 186: the verdict is judged", [c3, invalidation(1, c3), invalidation(2, c3, candidate=1, thesis_id=REVISION)], at(2, "payload.candidate")),
        case("names_no_verdict", "rule 186: the call has a verdict at the candidate", [c1, invalidation(1, c1), invalidation(2, c1, candidate=5, thesis_id=REVISION)], at(2, "payload.candidate")),
        case("one_verdict_twice", "rule 186: one record per verdict", [c1, invalidation(1, c1), invalidation(2, c1, thesis_id=REVISION)], at(2, "payload.candidate")),
        case("a_judged_verdict_with_no_record", "rule 186: every judged verdict is handed on", [c1], at(0, "payload.verdicts[0].result")),
        case("the_second_verdict_with_no_record", "rule 186: every judged verdict, in order", [c2, invalidation(1, c2)], at(0, "payload.verdicts[1].result")),
    ]


def stream_cases() -> list[dict]:
    def case(name, clause, batches, expect):
        return {"name": name, "clause": clause, "batches": batches, "expect": expect}

    ok = {"outcome": "Valid"}

    def at(index, path):
        return {"outcome": "Invalid", "index": index, "reason": "schema", "path": path}

    admitted, revised, refused = DRAFTS[BASE_ADMITTED], DRAFTS[BASE_REVISED], DRAFTS[BASE_REFUSED]
    c1, c2 = call(1), call(2)
    both = [gateway.verdict(0, "thesis", "judged"), gateway.verdict(1, "invalidation", "judged")]
    c3, c4 = call(3, both), call(4, both)
    lineage = revised["payload"]["lineage_id"]
    return [
        case("invalidated_after_admission", "rule 187: an admitted thesis", [[admitted], [c1, invalidation(1, c1)]], ok),
        case("a_revision_invalidated", "rule 187: the revision holds its instrument", [[admitted], [revised], [c1, invalidation(1, c1, thesis_id=REVISION, lineage_id=lineage)]], ok),
        case("a_revoked_source", "rule 187: a revoked source's invalidation too", [[admitted], [invalidation(1, None)]], ok),
        case("re_admitted_after_its_invalidation", "rule 187: an invalidated instrument may be admitted again, in a new lineage", [[admitted], [c1, invalidation(1, c1)], [new_thesis()]], ok),
        case("a_re_admission_invalidated", "rule 187: a re-admission is a new thesis, invalidated once too", [[admitted], [c1, invalidation(1, c1)], [new_thesis()], [c2, invalidation(2, c2, thesis_id=RENEWAL, lineage_id=RENEWAL)]], ok),
        case("invalidated_then_a_new_thesis_in_one_response", "§9.22 batch order: the invalidation follows the call and precedes the response's thesis records", [[admitted], [c3, invalidation(1, c3, candidate=1), new_thesis()]], ok),
        case("in_one_batch", "rule 187: the admission is earlier in the batch", [[admitted, c1, invalidation(1, c1)]], ok),
        case("never_admitted", "rule 187: a thesis the stream admitted", [[c1, invalidation(1, c1)]], at(1, "payload.thesis_id")),
        case("before_its_admission", "rule 187: admitted earlier", [[c1, invalidation(1, c1), admitted]], at(1, "payload.thesis_id")),
        case("a_refused_thesis", "rule 187: admitted, not only judged", [[refused], [c1, invalidation(1, c1, thesis_id=REFUSED_THESIS, lineage_id=REFUSED_THESIS, instrument_id=OTHER_ASSET)]], at(2, "payload.thesis_id")),
        case("invalidated_twice", "rule 187: one invalidation per thesis", [[admitted], [c1, invalidation(1, c1)], [c2, invalidation(2, c2)]], at(4, "payload.thesis_id")),
        case("revoked_after_its_invalidation", "rule 187: whatever the second's cause", [[admitted], [c1, invalidation(1, c1)], [invalidation(2, None)]], at(3, "payload.thesis_id")),
        case("another_lineage", "rule 187: the thesis's own lineage", [[admitted], [c1, invalidation(1, c1, lineage_id="th_01J8ZTZZ")]], at(2, "payload.lineage_id")),
        case("a_substituted_instrument", "rule 187: the thesis's own instrument", [[admitted], [c1, invalidation(1, c1, instrument_id=OTHER_ASSET)]], at(2, "payload.instrument_id")),
        case("superseded_by_its_revision", "rule 187: the thesis that holds the instrument now", [[admitted], [revised], [c1, invalidation(1, c1)]], at(3, "payload.thesis_id")),
        case("a_new_thesis_before_its_invalidation_a_correct_writer_never_uses", "§9.22 batch order: a thesis record before the invalidation supersedes it", [[admitted], [c4, new_thesis(), invalidation(1, c4, candidate=1)]], at(3, "payload.thesis_id")),
        case("superseded_by_a_renewal", "rule 187: a renewal holds it now", [[admitted], [renewal()], [c1, invalidation(1, c1)]], at(3, "payload.thesis_id")),
    ]


# --------------------------------------------------------------------------- independent checks

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.linked",
    "invalid_drafts",
    "valid_drafts",
    "batches.records",
    "batches.rule",
    "streams.records",
    "streams.batches",
    "streams.rule",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def expected_failure(case: dict) -> dict | None:
    e = case["expect"]
    return None if e["outcome"] == "Valid" else {k: e[k] for k in ("index", "reason", "path")}


def check_section(section: dict, skip: frozenset[str] = frozenset()) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    drafts = section["drafts"]
    for name, draft in drafts.items():
        got = violations(draft, skip)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    m, a = drafts[BASE_INVALIDATED], drafts[BASE_ADMITTED]
    linked = (
        m["causation_id"] == drafts[BASE_CALL]["event_id"]
        and all(m["payload"][k] == a["payload"][k] for k in ("thesis_id", "lineage_id", "instrument_id"))
        and REVOKED_SOURCE in a["payload"]["evidence_sources"]
        and drafts[BASE_REVOKED]["payload"]["source"] == REVOKED_SOURCE
    )
    if not linked:
        problems.append(found("drafts.linked", "the base invalidations do not name the base call and the admitted thesis"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case), skip)
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case), skip)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    for case in section["batches"]:
        for i, record in enumerate(case["drafts"]):
            got = violations(record, skip)
            if got:
                problems.append(found("batches.records", f"{case['name']} draft {i}: {got}"))
        got = batch_failure(case["drafts"], skip)
        if got != expected_failure(case):
            problems.append(found("batches.rule", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["streams"]:
        records = flat(case)
        for i, record in enumerate(records):
            got = violations(record, skip)
            if got:
                problems.append(found("streams.records", f"{case['name']} record {i}: {got}"))
        for batch in case["batches"]:
            got = batch_failure(batch, skip) or gateway.agent_stream_failure(records, skip)
            if got is not None:
                problems.append(found("streams.batches", f"{case['name']}: a batch breaks rule 186 or the stream rule 153: {got}"))
                break
        got = stream_failure(records, skip)
        if got != expected_failure(case):
            problems.append(found("streams.rule", f"{case['name']}: expected {case['expect']}, got {got}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "record.extra",
    "record.missing",
    "config_refs.required",
    *(f"rule.184.{n}" for n in ("candidate", "evidence", "source", "causation")),
    "rule.185",
    *(f"batch.186.{n}" for n in ("complete", "order", "call", "present", "kind", "result", "once")),
    *(f"stream.187.{n}" for n in ("admitted", "once", "known", "lineage", "instrument", "current")),
)


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    return [
        ("a base invalidation names no mandate", "drafts.valid", mutated(lambda s: s["drafts"][BASE_INVALIDATED].update(config_refs={}))),
        ("the base invalidation names another thesis", "drafts.linked", mutated(lambda s: s["drafts"][BASE_INVALIDATED]["payload"].update(thesis_id=REVISION))),
        ("an invalid draft's path differs", "invalid_drafts", mutated(lambda s: case(s, "invalid_drafts", "revoked.184.no_source")["expect"].update(path="payload.candidate"))),
        ("a valid draft names a source", "valid_drafts", mutated(lambda s: case(s, "valid_drafts", "invalidated.no_evidence")["changes"].append(change("payload.source", REVOKED_SOURCE)))),
        ("a batch draft is the executor's", "batches.records", mutated(lambda s: case(s, "batches", "a_revoked_source")["drafts"][0].update(actor=dict(SERVICES)))),
        ("a batch case expects the wrong draft", "batches.rule", mutated(lambda s: case(s, "batches", "one_verdict_twice")["expect"].update(index=1))),
        ("a stream record is the executor's", "streams.records", mutated(lambda s: case(s, "streams", "a_revoked_source")["batches"][1][0].update(actor=dict(SERVICES)))),
        ("a stream batch drops its invalidation", "streams.batches", mutated(lambda s: case(s, "streams", "invalidated_after_admission")["batches"][1].pop())),
        ("a stream case expects the wrong record", "streams.rule", mutated(lambda s: case(s, "streams", "invalidated_twice")["expect"].update(index=2))),
    ]


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught; a vector mutant only by the check it is registered against."""
    escaped = []
    for mutant in VALIDATOR_MUTANTS:
        if not check_section(section, frozenset([mutant])):
            escaped.append(f"invalidation mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"invalidation check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"invalidation vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": copy.deepcopy(DRAFTS),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "batches": batch_cases(),
        "streams": stream_cases(),
    }


__all__ = ["build_section", "check_section", "run_mutants", "vector_mutants"]
