"""Journal spec v0.44 §9.17's reference vectors for the control-plane records (DEC-981): the
workspace deployment's records of its link to the global control plane (control-plane design
§3.8), written by workspace control services on the control stream, never by `cp-agent`.

The schemas and rules 137 to 142 live in `control.py`, beside the other closed schemas, so one
validator judges every record. This module builds the `control_plane` section: a base draft of
each record, an invalid draft for every member type and every rule, and valid drafts for the cases
a rule might be misread to refuse. Its own oracle reads the base drafts' payloads directly, without
the validator, and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
import re

from common import change, delete
from control import (
    AGENT_STREAM,
    CP_RECORDS,
    SCHEMAS,
    SERVICES,
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

SPEC = "docs/specs/journal.md v0.44 §9.17 (DEC-981)"
AT = "2026-10-17T13:05:00.000000000Z"
EARLIER = "2026-10-01T00:00:00.000000000Z"
LATER = "2027-10-01T00:00:00.000000000Z"
DEPLOYMENT = "dep_01JCELL1"
OTHER_DEPLOYMENT = "dep_01JCELL2"
ORG = "org_01JACME"
CAUSE = "01J8Z3P2A000000000000000Q3"
GENESIS = "0" * 64
OPAQUE = re.compile(r"^[A-Za-z0-9_-]{1,64}\Z")
INSTANT = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{9}Z\Z")


def hexdigest(fill: str) -> str:
    return fill * 64


PAYLOADS: dict[str, dict] = {
    "ControlPlaneEnrolled": {
        "deployment_id": DEPLOYMENT,
        "org_id": ORG,
        "mode": "managed_cell",
        "certificate_fingerprint": hexdigest("a"),
    },
    "LicenseApplied": {
        "license_id": "lic_0001",
        "sequence": 3,
        "org_id": ORG,
        "deployment_ids": [DEPLOYMENT],
        "max_workspaces": 5,
        "max_agents_deployed": 20,
        "features": ["connected_clients"],
        "not_before": EARLIER,
        "not_after": LATER,
        "grace_days": 30,
        "key_id": "license_key_1",
        "document_digest": hexdigest("b"),
    },
    "LicenseStateChanged": {
        "license_id": "lic_0001",
        "from": None,
        "to": "valid",
        "judged_at": AT,
    },
    "ReleaseOffered": {
        "release_id": "rel_2026_10_17",
        "sequence": 12,
        "manifest_digest": hexdigest("c"),
        "key_id": "release_key_1",
    },
    "ReleaseInstalled": {
        "release_id": "rel_2026_10_17",
        "sequence": 12,
        "manifest_digest": hexdigest("c"),
        "trigger": "promotion",
    },
    "ReleaseWithdrawnNoticed": {
        "release_id": "rel_2026_10_10",
        "reason": "defect",
        "key_id": "release_key_1",
        "document_digest": hexdigest("d"),
    },
    "CatalogEntryRegistered": {
        "kind": "connector_package",
        "entry_id": "connector_robinhood",
        "content_hash": hexdigest("e"),
        "sequence": 4,
        "key_id": "release_key_1",
    },
    "DataBundleImported": {
        "dataset": "sec_filings",
        "bundle_digest": hexdigest("f"),
        "sequence": 101,
        "key_id": "data_key_1",
    },
    "UsageReportSealed": {
        "report_id": "rpt_0042",
        "deployment_id": DEPLOYMENT,
        "org_id": ORG,
        "period_start": "2026-10-17T12:00:00.000000000Z",
        "period_end": "2026-10-17T13:00:00.000000000Z",
        "prev_report_hash": hexdigest("1"),
        "report_digest": hexdigest("2"),
    },
    "ControlPlaneMessageRefused": {
        "message": "license",
        "reason": "replayed",
        "sequence": 2,
        "document_digest": hexdigest("3"),
    },
}
EVENT_IDS = {name: f"01J8Z4CP{i:02d}".ljust(26, "0") for i, name in enumerate(CP_RECORDS)}


def snake(record: str) -> str:
    """A record's base-draft key: §4.1's key grammar admits no capital letter."""
    return re.sub(r"(?<!^)(?=[A-Z])", "_", record).lower()


def base_drafts() -> dict[str, dict]:
    return {
        snake(name): {
            "envelope_version": 1,
            "environment": "paper",
            "event_id": EVENT_IDS[name],
            "stream_id": STREAM,
            "event_type": name,
            "schema_version": 1,
            "event_time": AT,
            "clock_source": "local",
            "causation_id": None,
            "correlation_id": None,
            "actor": dict(SERVICES),
            "config_refs": {},
            "payload": copy.deepcopy(PAYLOADS[name]),
            "artifact_refs": [],
            "pii_refs": [],
        }
        for name in CP_RECORDS
    }


def invalid(name, clause, base, changes, reason, path):
    return control_invalid(f"{base}.{name}", clause, snake(base), changes, reason, path)


def valid(name, clause, base, changes):
    return control_valid(f"{base}.{name}", clause, snake(base), changes)


def wrong_form(ty) -> object | None:
    """A string the member's type refuses as `non_canonical`, or `None` when a string is refused
    as `schema` (an integer) or nothing else fits."""
    kind = ty.inner.kind if ty.kind == "nullable" else ty.kind
    return {
        "id": "not an id",
        "one_of": "not_listed",
        "ts": "2026-10-17",
        "digest": "A" * 64,
    }.get(kind)


def member_drafts() -> list[dict]:
    """Per record, per member: a value of the wrong JSON kind (`schema`), a string of the wrong
    form (`non_canonical`), `null` where the type is not nullable (`schema`), and per record one
    missing and one extra member."""
    out = []
    for record in CP_RECORDS:
        for member, ty in SCHEMAS[("ctl", record)].fields:
            path = f"payload.{member}"
            if ty.kind == "list":
                out.append(
                    invalid(f"{member}.kind", "§9.17 types", record, [change(path, "x")], "schema", path)
                )
                out.append(
                    invalid(
                        f"{member}.item",
                        "§9.17 types",
                        record,
                        [change(path, ["not an id"])],
                        "non_canonical",
                        f"{path}[0]",
                    )
                )
                continue
            inner = ty.inner if ty.kind == "nullable" else ty
            wrong_kind = "3" if inner.kind == "int" else 7
            out.append(
                invalid(f"{member}.kind", "§9.17 types", record, [change(path, wrong_kind)], "schema", path)
            )
            form = wrong_form(ty)
            if form is not None:
                out.append(
                    invalid(
                        f"{member}.form", "§9.17 types", record, [change(path, form)], "non_canonical", path
                    )
                )
            if ty.kind != "nullable":
                out.append(
                    invalid(f"{member}.null", "§9.17 types", record, [change(path, None)], "schema", path)
                )
        first = SCHEMAS[("ctl", record)].fields[0][0]
        out.append(
            invalid(
                "missing", "§9.17 closed", record, [delete(f"payload.{first}")], "schema", f"payload.{first}"
            )
        )
        out.append(
            invalid("extra", "§9.17 closed", record, [change("payload.note", "x")], "schema", "payload.note")
        )
    return out


def rule_drafts() -> list[dict]:
    out = []
    for record in CP_RECORDS:
        out.append(
            invalid(
                "by_a_user",
                "rule 137: workspace control services write it as a system actor",
                record,
                [change("actor", dict(USER))],
                "schema",
                "actor.kind",
            )
        )
        out.append(
            invalid(
                "on_an_agent_stream",
                "§9 row: a control-stream record",
                record,
                [change("stream_id", AGENT_STREAM)],
                "wrong_stream",
                "event_type",
            )
        )
        out.append(
            invalid(
                "at_version_2",
                "§9.17: closed at schema version 1 only",
                record,
                [change("schema_version", 2)],
                "unknown_schema",
                "payload",
            )
        )
    lic = "LicenseApplied"
    out += [
        invalid(
            "no_deployments",
            "rule 138: a license covers a deployment",
            lic,
            [change("payload.deployment_ids", [])],
            "schema",
            "payload.deployment_ids",
        ),
        invalid(
            "deployments_descending",
            "rule 138: deployments strictly ascending",
            lic,
            [change("payload.deployment_ids", [OTHER_DEPLOYMENT, DEPLOYMENT])],
            "non_canonical",
            "payload.deployment_ids",
        ),
        invalid(
            "deployments_repeated",
            "rule 138: deployments strictly ascending, so a repeat is refused",
            lic,
            [change("payload.deployment_ids", [DEPLOYMENT, DEPLOYMENT])],
            "non_canonical",
            "payload.deployment_ids",
        ),
        invalid(
            "features_descending",
            "rule 138: features strictly ascending",
            lic,
            [change("payload.features", ["hybrid", "connected_clients"])],
            "non_canonical",
            "payload.features",
        ),
        invalid(
            "validity_empty",
            "rule 138: not_before earlier than not_after",
            lic,
            [change("payload.not_after", EARLIER)],
            "schema",
            "payload.not_after",
        ),
        invalid(
            "validity_reversed",
            "rule 138: not_before earlier than not_after",
            lic,
            [change("payload.not_before", LATER), change("payload.not_after", EARLIER)],
            "schema",
            "payload.not_after",
        ),
    ]
    for record in (
        "LicenseApplied",
        "ReleaseOffered",
        "ReleaseInstalled",
        "CatalogEntryRegistered",
        "DataBundleImported",
        "ControlPlaneMessageRefused",
    ):
        out.append(
            invalid(
                "sequence_zero",
                "rule 139: a sequence is at least 1",
                record,
                [change("payload.sequence", 0)],
                "schema",
                "payload.sequence",
            )
        )
    out += [
        invalid(
            "unchanged",
            "rule 140: a state change changes the state",
            "LicenseStateChanged",
            [change("payload.from", "valid")],
            "schema",
            "payload.to",
        ),
        invalid(
            "empty_period",
            "rule 141: period_start earlier than period_end",
            "UsageReportSealed",
            [change("payload.period_end", PAYLOADS["UsageReportSealed"]["period_start"])],
            "schema",
            "payload.period_end",
        ),
        invalid(
            "unknown_with_sequence",
            "rule 142: a message outside the list has no sequence",
            "ControlPlaneMessageRefused",
            [change("payload.message", "unknown"), change("payload.reason", "out_of_list")],
            "schema",
            "payload.sequence",
        ),
        invalid(
            "unknown_unsigned",
            "rule 142: a message outside the list is refused only out_of_list or malformed",
            "ControlPlaneMessageRefused",
            [
                change("payload.message", "unknown"),
                change("payload.reason", "unsigned"),
                change("payload.sequence", None),
            ],
            "schema",
            "payload.reason",
        ),
        invalid(
            "replayed_without_sequence",
            "rule 142: a replay is judged by its sequence",
            "ControlPlaneMessageRefused",
            [change("payload.sequence", None)],
            "schema",
            "payload.sequence",
        ),
    ]
    return out


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every member type and rule."""
    return [*member_drafts(), *rule_drafts()]


def valid_drafts() -> list[dict]:
    """The cases a rule might be misread to refuse; each must be accepted."""
    lic, refused = "LicenseApplied", "ControlPlaneMessageRefused"
    return [
        valid("hybrid", "§9.17: either mode", "ControlPlaneEnrolled", [change("payload.mode", "hybrid")]),
        valid(
            "two_deployments",
            "rule 138: several deployments, ascending",
            lic,
            [change("payload.deployment_ids", [DEPLOYMENT, OTHER_DEPLOYMENT])],
        ),
        valid("no_features", "rule 138: the feature set may be empty", lic, [change("payload.features", [])]),
        valid(
            "both_features",
            "rule 138: both features, ascending",
            lic,
            [change("payload.features", ["connected_clients", "hybrid"])],
        ),
        valid("sequence_one", "rule 139: the first sequence is 1", lic, [change("payload.sequence", 1)]),
        valid(
            "zero_entitlements",
            "§9.17: an entitlement may be zero",
            lic,
            [
                change("payload.max_workspaces", 0),
                change("payload.max_agents_deployed", 0),
                change("payload.grace_days", 0),
            ],
        ),
        valid(
            "lapsed",
            "rule 140: any move between states",
            "LicenseStateChanged",
            [change("payload.from", "grace"), change("payload.to", "lapsed")],
        ),
        valid(
            "renewed",
            "rule 140: a lapse ends with a new license",
            "LicenseStateChanged",
            [change("payload.from", "lapsed"), change("payload.to", "valid")],
        ),
        valid(
            "rollback",
            "§9.17: an operator rollback names an older release",
            "ReleaseInstalled",
            [change("payload.trigger", "rollback"), change("payload.sequence", 1)],
        ),
        valid(
            "registry_entry",
            "§9.17: either catalog kind",
            "CatalogEntryRegistered",
            [change("payload.kind", "model_registry_entry")],
        ),
        valid(
            "first_report",
            "§9.17: a deployment's first report chains from 64 zeros",
            "UsageReportSealed",
            [change("payload.prev_report_hash", GENESIS)],
        ),
        valid(
            "unreadable",
            "rule 142: a refusal with no readable sequence",
            refused,
            [change("payload.sequence", None), change("payload.reason", "malformed")],
        ),
        valid(
            "unknown",
            "rule 142: a message outside the list",
            refused,
            [
                change("payload.message", "unknown"),
                change("payload.reason", "out_of_list"),
                change("payload.sequence", None),
            ],
        ),
        valid(
            "caused", "§9.17: no rule reads the cause", "LicenseStateChanged", [change("causation_id", CAUSE)]
        ),
        valid("live", "§9.17: either environment", "ReleaseOffered", [change("environment", "live")]),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.records",
    "drafts.no_text",
    "drafts.sequence",
    "invalid_drafts",
    "valid_drafts",
)
SEQUENCED = (
    "LicenseApplied",
    "ReleaseOffered",
    "ReleaseInstalled",
    "CatalogEntryRegistered",
    "DataBundleImported",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def plain_strings(value) -> list[str]:
    """Every string in a payload that is not an ID, a digest, a timestamp, or a listed value: the
    oracle's own reading of CP-1, done without the validator's types."""
    if isinstance(value, dict):
        return [s for v in value.values() for s in plain_strings(v)]
    if isinstance(value, list):
        return [s for v in value for s in plain_strings(v)]
    if isinstance(value, str) and not (OPAQUE.match(value) or INSTANT.match(value)):
        return [value]
    return []


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    drafts = section["drafts"]
    if sorted(drafts) != sorted(snake(r) for r in CP_RECORDS):
        problems.append(
            found("drafts.records", f"base drafts {sorted(drafts)} are not the ten §9.17 records")
        )
    records = {snake(r): r for r in CP_RECORDS}
    for name, draft in drafts.items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
        if draft.get("event_type") != records.get(name) or draft.get("actor", {}).get("kind") != "system":
            problems.append(
                found("drafts.records", f"base draft {name} is not that record by a system actor")
            )
        if plain_strings(draft.get("payload")):
            problems.append(found("drafts.no_text", f"base draft {name} carries text"))
        sequence = (draft.get("payload") or {}).get("sequence")
        if records.get(name) in SEQUENCED and not (isinstance(sequence, int) and sequence >= 1):
            problems.append(found("drafts.sequence", f"base draft {name} has no sequence of at least 1"))
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
    "record.extra",
    "record.missing",
    "rule.137",
    "rule.138.empty",
    "rule.138.deployments",
    "rule.138.features",
    "rule.138.validity",
    "rule.139",
    "boundary.rule_139",
    "rule.140",
    "rule.141",
    "rule.142.reason",
    "rule.142.sequence",
    "rule.142.replayed",
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
            "a base draft is written by a user",
            "drafts.valid",
            mutated(lambda s: s["drafts"]["license_applied"].update(actor=dict(USER))),
        ),
        (
            "a base draft is filed as another record",
            "drafts.records",
            mutated(lambda s: s["drafts"]["usage_report_sealed"].update(event_type="ReleaseOffered")),
        ),
        (
            "a base draft carries free text in an id-shaped member",
            "drafts.no_text",
            mutated(lambda s: s["drafts"]["release_offered"]["payload"].update(release_id="x" * 65)),
        ),
        (
            "a base draft carries no sequence",
            "drafts.sequence",
            mutated(lambda s: s["drafts"]["data_bundle_imported"]["payload"].update(sequence=0)),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(
                lambda s: case(s, "invalid_drafts", "LicenseStateChanged.unchanged")["expect"].update(
                    path="payload.from"
                )
            ),
        ),
        (
            "a valid draft breaks rule 141",
            "valid_drafts",
            mutated(
                lambda s: case(s, "valid_drafts", "UsageReportSealed.first_report")["changes"].append(
                    change("payload.period_start", "2026-10-17T14:00:00.000000000Z")
                )
            ),
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
            escaped.append(f"control-plane validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"control-plane check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(
                f"control-plane vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})"
            )
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
