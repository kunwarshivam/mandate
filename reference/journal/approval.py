"""Journal spec v0.17 §9.7's reference vectors (DEC-533): the owner's approval answer on the control
stream (`ApprovalResponseSubmitted`) and the runtime's two records of it on the agent stream
(`ApprovalResponded`, `ApprovalRevalidated`).

The schemas and rules 46 to 53 live in `control.py`, beside §9.2's to §9.5's, so one validator
judges every closed schema. This module builds the `approval_answers` section: a base draft of each
record, an invalid draft for every member type and rule, and valid drafts for the cases a rule might
be misread to refuse. It checks the section with two oracles of its own: each `ApprovalResponded`
copies its answer's members exactly, and each `act`'s drift is recomputed with exact fractions,
independently of rule 53's decimal comparison. Every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
from fractions import Fraction

from common import change, delete, digest_strings
from control import (
    AGENT,
    AGENT_STREAM,
    OWNER,
    RUNTIME,
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

SPEC = "docs/specs/journal.md v0.17 §9.7 (DEC-533)"
AT = "2026-09-21T13:00:00.000000000Z"
APPROVAL = "01J8Z3P0A000000000000000Q1"
IDS = {
    "answer": "01J8Z3P1A000000000000000Q2",
    "responded": "01J8Z3P2A000000000000000Q3",
    "revalidated": "01J8Z3P3A000000000000000Q4",
}
CONTENT_HASH = "sha256:" + "a" * 64
BOUND = "sha256:" + "1" * 64
LATER = "sha256:" + "2" * 64
SUBMITTED_AT = 1790000000
STEP_UP = {"assertion_id": "assert_owner_02", "authenticated_at": 1789999990, "method": "cli_confirm"}
COPIED = ("approval", "verdict", "responder", "role", "step_up")


def envelope(name: str, event_type: str, stream: str, actor: dict, payload: dict, causation=None) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": IDS[name],
        "stream_id": stream,
        "event_type": event_type,
        "schema_version": 1,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": causation,
        "correlation_id": None,
        "actor": dict(actor),
        "config_refs": {} if stream == STREAM else {"mandate_version": BOUND},
        "payload": payload,
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    answer = {
        "agent": AGENT,
        "approval": APPROVAL,
        "verdict": "approved",
        "content_hash": CONTENT_HASH,
        "submitted_at": SUBMITTED_AT,
        "step_up": dict(STEP_UP),
        "responder": OWNER,
        "role": "approver",
    }
    responded = {
        "approval": APPROVAL,
        "verdict": "approved",
        "responder": OWNER,
        "role": "approver",
        "result": "admitted",
        "reason": None,
        "effective_at": SUBMITTED_AT + 1,
        "step_up": dict(STEP_UP),
        "quorum": {"independent": False, "required": 1},
        "separation_of_duties": None,
        "delegation": None,
    }
    revalidated = {
        "approval": APPROVAL,
        "result": "act",
        "reason": None,
        "mandate_version_bound": BOUND,
        "mandate_version_now": BOUND,
        "mode": "normal",
        "instrument_restricted": False,
        "decided_by_bound": "rule:open",
        "decided_by_now": "rule:open",
        "dry_run": "allow",
        "dry_run_reason": None,
        "m_req": "149.5",
        "m_now": "150.5",
        "band_bp": 100,
    }
    return {
        "answer_approved": envelope("answer", "ApprovalResponseSubmitted", STREAM, USER, answer),
        "responded_admitted": envelope(
            "responded", "ApprovalResponded", AGENT_STREAM, RUNTIME, responded, IDS["answer"]
        ),
        "revalidated_act": envelope(
            "revalidated", "ApprovalRevalidated", AGENT_STREAM, RUNTIME, revalidated, IDS["responded"]
        ),
    }


BASES = base_drafts()


def refs_kept(base: str, changes: list[dict]) -> list[dict]:
    """`changes`, then `artifact_refs` set to the changed payload's digests (§3), so a case that edits
    a reference member breaks only the rule it names."""
    draft = copy.deepcopy(BASES[base])
    for item in changes:
        draft_change(draft, item)
    return [*changes, change("artifact_refs", sorted(digest_strings(draft["payload"])))]


def draft_change(draft: dict, item: dict) -> None:
    *parents, last = item["path"].split(".")
    node = draft
    for name in parents:
        node = node[name]
    if item.get("delete"):
        del node[last]
    else:
        node[last] = copy.deepcopy(item["value"])


def invalid(name, clause, base, changes, reason, path, also=()):
    return control_invalid(name, clause, base, refs_kept(base, changes), reason, path, also)


def valid(name, clause, base, changes):
    return control_valid(name, clause, base, refs_kept(base, changes))


# Each member, a value of the wrong JSON kind (`schema`) and, where its type constrains a string, a
# string of the wrong form (`non_canonical`).
MEMBER_CASES = {
    "answer_approved": (
        ("agent", 7, "agent:a"),
        ("approval", 7, "01J8Z3P0A000000000000000QI"),
        ("verdict", True, "denied"),
        ("content_hash", 7, "sha256:" + "A" * 64),
        ("submitted_at", "1790000000", None),
        ("step_up", "cli_confirm", None),
        ("responder", 7, ""),
        ("role", 7, "author"),
    ),
    "responded_admitted": (
        ("approval", 7, "not-a-ulid"),
        ("verdict", 7, "denied"),
        ("responder", 7, ""),
        ("role", 7, "author"),
        ("result", 7, "recorded"),
        ("reason", 7, "expired"),
        ("effective_at", "2026-09-21T13:00:01.000000000Z", None),
        ("step_up", [], None),
        ("quorum", 2, None),
        ("separation_of_duties", "independent", None),
        ("delegation", {"shape": "one_order"}, None),
    ),
    "revalidated_act": (
        ("approval", 7, "not-a-ulid"),
        ("result", 7, "acted"),
        ("reason", 7, ""),
        ("mandate_version_bound", 7, "v1"),
        ("mandate_version_now", 7, "v1"),
        ("mode", 7, "halted"),
        ("instrument_restricted", "false", None),
        ("decided_by_bound", 7, ""),
        ("decided_by_now", 7, ""),
        ("dry_run", 7, "denied"),
        ("dry_run_reason", 7, ""),
        ("m_req", 149, "149,5"),
        ("m_now", 150, "1.5.0"),
        ("band_bp", "100", None),
    ),
}
# A re-validation's type cases start from a skip, so no rule 53 check also refuses the edited member
# and a seeded bug that loosens a type check cannot hide behind one.
TYPE_BASE = {"revalidated_act": [change("payload.result", "skip"), change("payload.reason", "drift")]}
# `band_bp`'s value set is rule 53's, at the member's own path with the same reason, so a draft with
# a mistyped or null band is refused identically whether or not its type is checked first.
RULE_TYPED = ("band_bp",)
# Non-nullable members a `null` must not satisfy, each its own case.
NULLED = {
    "answer_approved": ("agent", "approval", "verdict", "content_hash", "submitted_at", "responder", "role"),
    "responded_admitted": ("approval", "verdict", "responder", "role", "result", "effective_at"),
    "revalidated_act": (
        "approval",
        "result",
        "mandate_version_bound",
        "mandate_version_now",
        "mode",
        "instrument_restricted",
        "decided_by_bound",
        "dry_run",
        "band_bp",
    ),
}


def member_drafts() -> list[dict]:
    out = []
    for base, cases in MEMBER_CASES.items():
        first = TYPE_BASE.get(base, [])
        for member, wrong_kind, wrong_form in cases:
            path = f"payload.{member}"
            kind = [*first, change(path, wrong_kind)]
            out.append(invalid(f"{base}.{member}.kind", "§9.7 types", base, kind, "schema", path))
            if wrong_form is not None:
                form = [*first, change(path, wrong_form)]
                out.append(invalid(f"{base}.{member}.form", "§9.7 types", base, form, "non_canonical", path))
        for member in NULLED[base]:
            path = f"payload.{member}"
            nulled = [*first, change(path, None)]
            out.append(invalid(f"{base}.{member}.null", "§9.7 types", base, nulled, "schema", path))
        some = MEMBER_CASES[base][0][0]
        out.append(invalid(f"{base}.missing", "§9.7 closed", base, [delete(f"payload.{some}")], "schema", f"payload.{some}"))
        out.append(invalid(f"{base}.extra", "§9.7 closed", base, [change("payload.note", "x")], "schema", "payload.note"))
    return out


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule, or the rules its `also` lists; together they cover every
    §9.7 member type and rule."""
    reval = "revalidated_act"
    resp = "responded_admitted"
    return [
        *member_drafts(),
        invalid(
            "answer_authenticated_at_as_a_timestamp",
            "§9.7: times are integer risk-clock seconds, never a timestamp (DEC-533 item 2)",
            "answer_approved",
            [change("payload.step_up.authenticated_at", "2026-09-21T12:59:50.000000000Z")],
            "schema",
            "payload.step_up.authenticated_at",
        ),
        invalid(
            "answer_step_up_extra",
            "§9.7: the step-up evidence is closed",
            "answer_approved",
            [change("payload.step_up.note", "x")],
            "schema",
            "payload.step_up.note",
        ),
        invalid(
            "quorum_required_as_text",
            "§9.7: `quorum.required` is an integer",
            resp,
            [change("payload.quorum.required", "1")],
            "schema",
            "payload.quorum.required",
        ),
        invalid(
            "answer_wrong_stream",
            "§9.7: the answer is a control-stream record",
            "answer_approved",
            [change("stream_id", AGENT_STREAM)],
            "wrong_stream",
            "event_type",
        ),
        invalid(
            "responded_without_its_mandate",
            "§9 row: `ApprovalResponded` names `mandate_version`",
            resp,
            [change("config_refs", {})],
            "missing_config_ref",
            "config_refs.mandate_version",
        ),
        invalid(
            "revalidated_refs_not_listed",
            "§3: `artifact_refs` lists the payload's references",
            reval,
            [],
            "artifact_refs",
            "artifact_refs",
        )
        | {"changes": [change("artifact_refs", [])]},
        invalid(
            "answer_responder_not_the_writer",
            "rule 46",
            "answer_approved",
            [change("payload.responder", "user_other")],
            "schema",
            "payload.responder",
        ),
        invalid(
            "admitted_with_a_reason",
            "rule 47: a reason exactly when refused",
            resp,
            [change("payload.reason", "late")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "refused_without_a_reason",
            "rule 47",
            resp,
            [change("payload.result", "refused"), change("payload.quorum", None)],
            "schema",
            "payload.reason",
        ),
        invalid(
            "admitted_without_a_quorum",
            "rule 48: check 7 was judged",
            resp,
            [change("payload.quorum", None)],
            "schema",
            "payload.quorum",
        ),
        invalid(
            "late_with_a_quorum",
            "rule 48: check 7 was never reached",
            resp,
            [change("payload.result", "refused"), change("payload.reason", "late")],
            "schema",
            "payload.quorum",
        ),
        invalid(
            "skip_with_a_quorum",
            "rule 48: a skip never reaches check 7",
            resp,
            [change("payload.verdict", "skipped"), change("payload.step_up", None)],
            "schema",
            "payload.quorum",
        ),
        invalid(
            "skip_counted",
            "rule 49: one admitted skip ends the approval",
            resp,
            [
                change("payload.verdict", "skipped"),
                change("payload.result", "counted"),
                change("payload.quorum", None),
                change("payload.step_up", None),
            ],
            "schema",
            "payload.result",
        ),
        invalid(
            "skip_refused_for_its_step_up",
            "rule 49: a skip runs checks 1 to 5 only",
            resp,
            [
                change("payload.verdict", "skipped"),
                change("payload.result", "refused"),
                change("payload.reason", "step_up_missing"),
                change("payload.quorum", None),
                change("payload.step_up", None),
            ],
            "schema",
            "payload.reason",
        ),
        invalid(
            "responded_without_its_answer",
            "rule 50: the copy names the answer it copies",
            resp,
            [change("causation_id", None)],
            "schema",
            "causation_id",
        ),
        invalid("act_with_a_reason", "rule 51", reval, [change("payload.reason", "drift")], "schema", "payload.reason"),
        invalid(
            "skip_without_a_reason",
            "rule 51",
            reval,
            [change("payload.result", "skip"), change("payload.mode", "paused")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "deny_without_its_reason",
            "rule 52",
            reval,
            [change("payload.result", "skip"), change("payload.reason", "buying_power"), change("payload.dry_run", "deny")],
            "schema",
            "payload.dry_run_reason",
        ),
        invalid(
            "allow_with_a_reason",
            "rule 52",
            reval,
            [change("payload.dry_run_reason", "buying_power")],
            "schema",
            "payload.dry_run_reason",
        ),
        invalid("band_not_a_band", "rule 53: 100 or 200", reval, [change("payload.band_bp", 150)], "schema", "payload.band_bp"),
        invalid(
            "act_on_another_version",
            "rule 53: check 8",
            reval,
            [change("payload.mandate_version_now", LATER)],
            "schema",
            "payload.mandate_version_now",
        ),
        invalid("act_while_paused", "rule 53: check 9", reval, [change("payload.mode", "paused")], "schema", "payload.mode"),
        invalid(
            "act_on_a_restricted_instrument",
            "rule 53: check 9",
            reval,
            [change("payload.instrument_restricted", True)],
            "schema",
            "payload.instrument_restricted",
        ),
        invalid(
            "act_on_another_trigger",
            "rule 53: check 10",
            reval,
            [change("payload.decided_by_now", "rule:other")],
            "schema",
            "payload.decided_by_now",
        ),
        invalid(
            "act_on_a_denied_dry_run",
            "rule 53: check 11",
            reval,
            [change("payload.dry_run", "deny"), change("payload.dry_run_reason", "buying_power")],
            "schema",
            "payload.dry_run",
        ),
        invalid("act_without_m_req", "rule 53: check 12", reval, [change("payload.m_req", None)], "schema", "payload.m_req"),
        invalid("act_without_m_now", "rule 53: check 12", reval, [change("payload.m_now", None)], "schema", "payload.m_now"),
        invalid(
            "act_drifted_past_the_band",
            "rule 53: check 12, one hundredth past the band",
            reval,
            [change("payload.m_req", "100"), change("payload.m_now", "101.01")],
            "schema",
            "payload.m_now",
        ),
        invalid(
            "act_reports_its_first_failure",
            "rule 53: in order, the version before the mode",
            reval,
            [change("payload.mandate_version_now", LATER), change("payload.mode", "paused")],
            "schema",
            "payload.mandate_version_now",
        ),
    ]


def valid_drafts() -> list[dict]:
    """Cases a rule might be misread to refuse; each must be accepted."""
    resp = "responded_admitted"
    reval = "revalidated_act"
    skipped = [change("payload.verdict", "skipped"), change("payload.step_up", None)]
    return [
        valid("answer_skipped", "§9.7: a skip carries no step-up", "answer_approved", skipped),
        valid(
            "answer_approved_without_step_up",
            "§9.7: the runtime refuses it as step_up_missing (check 6); the journal accepts it",
            "answer_approved",
            [change("payload.step_up", None)],
        ),
        valid(
            "refused_late",
            "rules 47 and 48: a refusal before check 7 has no quorum",
            resp,
            [change("payload.result", "refused"), change("payload.reason", "late"), change("payload.quorum", None)],
        ),
        valid(
            "counted",
            "rule 48: a counted grant was judged at check 7",
            resp,
            [change("payload.result", "counted"), change("payload.quorum", {"independent": True, "required": 2})],
        ),
        valid(
            "refused_not_independent",
            "rule 48: check 7 refused it, so its quorum is recorded",
            resp,
            [change("payload.result", "refused"), change("payload.reason", "not_independent")],
        ),
        valid("skip_admitted", "rules 48 and 49", resp, [*skipped, change("payload.quorum", None)]),
        valid(
            "skip_refused_content_mismatch",
            "rule 49: check 5 applies to a skip",
            resp,
            [
                *skipped,
                change("payload.quorum", None),
                change("payload.result", "refused"),
                change("payload.reason", "content_mismatch"),
            ],
        ),
        valid(
            "skip_on_a_denied_dry_run",
            "rules 51 and 52: a skip may record any failure",
            reval,
            [
                change("payload.result", "skip"),
                change("payload.reason", "buying_power"),
                change("payload.dry_run", "deny"),
                change("payload.dry_run_reason", "buying_power"),
                change("payload.mandate_version_now", LATER),
                change("payload.m_now", None),
            ],
        ),
        valid(
            "act_at_the_band_edge",
            "rule 53: inside includes the band itself",
            reval,
            [change("payload.m_req", "100"), change("payload.m_now", "101")],
        ),
        valid(
            "act_on_a_crypto_band_below",
            "rule 53: the band is symmetric, at 200 bp",
            reval,
            [change("payload.band_bp", 200), change("payload.m_req", "100"), change("payload.m_now", "98")],
        ),
        valid(
            "act_reclassified_auto",
            "rule 53: an `auto` re-classification passes check 10, its label null (§4.2)",
            reval,
            [change("payload.decided_by_now", None)],
        ),
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = ("drafts.valid", "drafts.copied", "drafts.drift", "invalid_drafts", "valid_drafts")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def inside_band(payload: dict) -> bool:
    """Mandate spec §6.4's drift, on exact fractions rather than rule 53's decimals."""
    m_req, m_now = Fraction(payload["m_req"]), Fraction(payload["m_now"])
    return abs(m_now - m_req) / m_req <= Fraction(payload["band_bp"], 10000)


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    answer = section["drafts"]["answer_approved"]
    responded = section["drafts"]["responded_admitted"]
    if responded["causation_id"] != answer["event_id"] or any(
        responded["payload"][m] != answer["payload"][m] for m in COPIED
    ):
        problems.append(found("drafts.copied", "the base ApprovalResponded does not copy its answer"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
        payload = draft["payload"]
        if draft["event_type"] == "ApprovalRevalidated" and payload["result"] == "act" and not inside_band(payload):
            problems.append(found("drafts.drift", f"{case['name']}: an act outside its band"))
    revalidated = section["drafts"]["revalidated_act"]["payload"]
    if not inside_band(revalidated):
        problems.append(found("drafts.drift", "the base act is outside its band"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.46",
    "rule.47",
    "rule.48",
    "rule.49.counted",
    "rule.49.reason",
    "rule.50",
    "rule.51",
    "rule.52",
    "rule.53.band",
    "rule.53.mandate_version_now",
    "rule.53.mode",
    "rule.53.instrument_restricted",
    "rule.53.decided_by_now",
    "rule.53.dry_run",
    "rule.53.m_req",
    "rule.53.m_now",
    "rule.53.m_now_drift",
    "boundary.rule_53_strict",
    "types.null",
    "record.extra",
    "record.missing",
    "config_refs.required",
    "artifact_refs",
    *sorted(
        {f"loose.payload.{member}" for cases in MEMBER_CASES.values() for member, _, _ in cases}
        - {f"loose.payload.{member}" for member in RULE_TYPED}
    ),
    *sorted(
        {f"nullable.payload.{member}" for members in NULLED.values() for member in members}
        - {f"nullable.payload.{member}" for member in RULE_TYPED}
    ),
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
        (
            "a base draft breaks rule 47",
            "drafts.valid",
            mutated(lambda s: s["drafts"]["responded_admitted"]["payload"].update(reason="late")),
        ),
        (
            "the base copy names another responder than its answer",
            "drafts.copied",
            mutated(lambda s: s["drafts"]["answer_approved"]["payload"].update(responder="user_owner_02")
                    or s["drafts"]["answer_approved"]["actor"].update(id="user_owner_02")),
        ),
        (
            "a valid act is moved outside its band while rule 53 is read loosely",
            "drafts.drift",
            mutated(lambda s: s["drafts"]["revalidated_act"]["payload"].update(m_req="100", m_now="100.5", band_bp=100)
                    or s["drafts"]["revalidated_act"]["payload"].update(m_now="101.5")),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "admitted_with_a_reason")["expect"].update(path="payload.result")),
        ),
        (
            "a valid draft breaks a rule",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "counted")["changes"].append(change("payload.quorum", None))),
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
            escaped.append(f"approval validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"approval check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"approval vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
