"""Journal spec v0.68 rule 255's reference vectors (DEC-2621, E7-27): §9.8's `ConnectionRequested`
version 2 names how the credential is obtained and, for an Alpaca OAuth connect, the token's possible
breadth and the disclosure the owner confirmed.

Rule 255 lives in `control.py`, beside the other payload rules, so one validator judges every closed
schema. This module builds the `connection_requested` section: an OAuth and an API-key base draft
at version 2 and a base draft at version 1, an invalid draft for each member type of version 2 and
for each clause of rule 255, reported at the member the rule names and in the rule's order, and
valid drafts a rule might be misread to refuse. Its own oracle reads the drafts' members directly,
without the validator, and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
import re

from common import change
from connections import CONNECTION, OWNER, STEP_UP
from control import (
    ACCOUNT_STREAM_REF,
    SERVICES,
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

SPEC = "docs/specs/journal.md v0.68 §9.8 rule 255 (DEC-2621)"
AT = "2026-10-19T15:30:00.000000000Z"
OAUTH = "oauth_v2"
API_KEY = "api_key_v2"
V1 = "request_v1"
EVENT_IDS = {
    OAUTH: "01JAB1R0A000000000000000Q1",
    API_KEY: "01JAB1R0A000000000000000Q2",
    V1: "01JAB1R0A000000000000000Q3",
}
DOCUMENT = "alpaca_oauth_token_breadth"
BREADTH = {"other_environment": "may_reach", "disclosure": {"document": DOCUMENT, "version": "1"}}
RULE = "rule 255"
ID_GRAMMAR = re.compile(r"[A-Za-z0-9_-]+")


def request(name: str) -> dict:
    payload = {
        "connection_id": CONNECTION,
        "account_ref": ACCOUNT_STREAM_REF,
        "broker": "alpaca",
        "environment": "paper",
        "user": OWNER,
        "step_up": dict(STEP_UP),
    }
    if name == OAUTH:
        payload |= {"credential": "oauth", "breadth": copy.deepcopy(BREADTH)}
    elif name == API_KEY:
        payload |= {"credential": "api_key", "breadth": None}
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": EVENT_IDS[name],
        "stream_id": STREAM,
        "event_type": "ConnectionRequested",
        "schema_version": 1 if name == V1 else 2,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": None,
        "correlation_id": None,
        "actor": dict(SERVICES),
        "config_refs": {},
        "payload": payload,
        "artifact_refs": [],
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    return {OAUTH: request(OAUTH), API_KEY: request(API_KEY), V1: request(V1)}


def invalid_drafts() -> list[dict]:
    """Each draft breaks one member type of version 2 or one clause of rule 255."""
    out = []

    def bad(name, clause, base, changes, reason, path):
        out.append(control_invalid(name, clause, base, changes, reason, path))

    types = "§9.8 v0.68 types"
    bad("credential_unknown", f"{types}: `credential` is `oauth` or `api_key`", OAUTH, [change("payload.credential", "key_pair")], "non_canonical", "payload.credential")
    bad("credential_null", f"{types}: `credential` is not nullable", OAUTH, [change("payload.credential", None)], "schema", "payload.credential")
    bad("breadth_not_object", f"{types}: `breadth` is an object or null", OAUTH, [change("payload.breadth", "may_reach")], "schema", "payload.breadth")
    bad(
        "reach_unknown",
        f"{types}: `other_environment` is `may_reach` while U-A4 is open",
        OAUTH,
        [change("payload.breadth.other_environment", "cannot_reach")],
        "non_canonical",
        "payload.breadth.other_environment",
    )
    bad(
        "disclosure_null",
        f"{types}: `disclosure` is not nullable",
        OAUTH,
        [change("payload.breadth.disclosure", None)],
        "schema",
        "payload.breadth.disclosure",
    )
    bad(
        "document_not_id",
        f"{types}: `document` is an id",
        OAUTH,
        [change("payload.breadth.disclosure.document", "alpaca oauth")],
        "non_canonical",
        "payload.breadth.disclosure.document",
    )
    bad(
        "version_not_text",
        f"{types}: `version` is text",
        OAUTH,
        [change("payload.breadth.disclosure.version", 1)],
        "schema",
        "payload.breadth.disclosure.version",
    )
    bad("oauth_without_breadth", f"{RULE}: an `oauth` request has a breadth", OAUTH, [change("payload.breadth", None)], "schema", "payload.breadth")
    bad("api_key_with_breadth", f"{RULE}: an `api_key` request has none", API_KEY, [change("payload.breadth", copy.deepcopy(BREADTH))], "schema", "payload.breadth")
    bad("oauth_robinhood", f"{RULE}: an `oauth` request names `alpaca`", OAUTH, [change("payload.broker", "robinhood")], "schema", "payload.broker")
    bad("oauth_live", f"{RULE}: an `oauth` request names `paper`", OAUTH, [change("payload.environment", "live")], "schema", "payload.environment")
    bad(
        "oauth_other_document",
        f"{RULE}: the disclosure is `{DOCUMENT}`",
        OAUTH,
        [change("payload.breadth.disclosure.document", "leveraged_etp")],
        "schema",
        "payload.breadth.disclosure.document",
    )
    bad(
        "oauth_live_other_document",
        f"{RULE}: reported at the first that fails: `payload.environment` before the document",
        OAUTH,
        [change("payload.environment", "live"), change("payload.breadth.disclosure.document", "leveraged_etp")],
        "schema",
        "payload.environment",
    )
    bad("v1_with_credential", "§9.8: version 1 is unchanged and closed", V1, [change("payload.credential", "api_key")], "schema", "payload.credential")
    bad("version_3", "§9.8: versions 1 and 2 only", OAUTH, [change("schema_version", 3)], "unknown_schema", "payload")
    return out


def valid_drafts() -> list[dict]:
    """Cases rule 255 might be misread to refuse."""
    return [
        control_valid("oauth_paper_alpaca", f"{RULE}: an Alpaca paper OAuth request with its breadth", OAUTH, []),
        control_valid("api_key_paper", f"{RULE}: an API-key request carries no breadth", API_KEY, []),
        control_valid("api_key_live", f"{RULE}: the broker and environment clauses bind only `oauth`", API_KEY, [change("payload.environment", "live")]),
        control_valid("api_key_robinhood", f"{RULE}: an `api_key` request of another broker", API_KEY, [change("payload.broker", "robinhood")]),
        control_valid("disclosure_version_2", f"{RULE}: any version of the document", OAUTH, [change("payload.breadth.disclosure.version", "2")]),
        control_valid("version_1", "§9.8: version 1 stays registered and unchanged", V1, []),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.shape",
    "invalid_drafts",
    "invalid_drafts.oracle",
    "valid_drafts",
    "valid_drafts.oracle",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def oracle_255(draft: dict) -> str | None:
    """Rule 255 read from the members alone: the member it reports, or None when it holds or does
    not apply (version 1, or a payload whose types §9.8 refuses first)."""
    p = draft["payload"]
    if draft["schema_version"] != 2 or p.get("credential") not in ("oauth", "api_key"):
        return None
    breadth = p.get("breadth")
    if breadth is not None and not isinstance(breadth, dict):
        return None
    if p["credential"] == "api_key":
        return None if breadth is None else "payload.breadth"
    if breadth is None:
        return "payload.breadth"
    if p.get("broker") != "alpaca":
        return "payload.broker"
    if p.get("environment") != "paper":
        return "payload.environment"
    disclosure = breadth.get("disclosure")
    document = disclosure.get("document") if isinstance(disclosure, dict) else None
    if isinstance(document, str) and ID_GRAMMAR.fullmatch(document) and document != DOCUMENT:
        return "payload.breadth.disclosure.document"
    return None


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
        p = draft["payload"]
        expected_version = 1 if name == V1 else 2
        shaped = draft["schema_version"] == expected_version and (
            name == V1 or (p["credential"] == "oauth") == (p["breadth"] is not None)
        )
        if not shaped:
            problems.append(found("drafts.shape", f"base draft {name} is not the request its name says"))
    for case in section["invalid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
        names_255 = case["clause"].startswith(RULE)
        if names_255 != (oracle_255(draft) == case["expect"]["path"]):
            problems.append(found("invalid_drafts.oracle", f"{case['name']}: the oracle disagrees on rule 255"))
    for case in section["valid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
        if oracle_255(draft) is not None:
            problems.append(found("valid_drafts.oracle", f"{case['name']}: the oracle refuses it by rule 255"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.255",
    "rule.255.paired",
    "rule.255.broker",
    "rule.255.environment",
    "rule.255.document",
    "order.rule_255_document_first",
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
        ("the OAuth base draft is at version 3", "drafts.valid", mutated(lambda s: s["drafts"][OAUTH].update(schema_version=3))),
        (
            "the API-key base draft is labelled OAuth with no breadth",
            "drafts.shape",
            mutated(lambda s: s["drafts"][API_KEY]["payload"].update(credential="oauth")),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "oauth_live")["expect"].update(path="payload.broker")),
        ),
        (
            "a rule-255 draft is relabelled as a type case",
            "invalid_drafts.oracle",
            mutated(lambda s: case(s, "invalid_drafts", "oauth_robinhood").update(clause="§9.8 v0.68 types")),
        ),
        (
            "a valid draft is an OAuth request without its breadth",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "oauth_paper_alpaca")["changes"].append(change("payload.breadth", None))),
        ),
        (
            "a valid draft is an API-key request with a breadth",
            "valid_drafts.oracle",
            mutated(lambda s: case(s, "valid_drafts", "api_key_live")["changes"].append(change("payload.breadth", copy.deepcopy(BREADTH)))),
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
            escaped.append(f"connection-requested validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"connection-requested check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"connection-requested vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
