"""Journal spec v0.24 §9.12's reference vectors (DEC-780): the control stream's `RecordsAccessed`,
`ExportCreated`, and `VerificationRun`.

The schemas and rules 96 to 101 live in `control.py`, beside the other closed schemas, so one
validator judges every closed schema. This module builds the `records_access` section: a base draft of each
record, an invalid draft for every member type and rule, and valid drafts for the cases a rule might
be misread to refuse. It checks the section with oracles of its own: every stream a valid draft
names is parsed for its workspace independently of rule 96, no valid payload carries a member
outside the closed vocabulary of streams, positions, hashes, codes, and opaque IDs, and each run's
result is recomputed from its ranges. Every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
import re

from common import apply_change, change, delete, digest_strings
from control import (
    AGENT_STREAM,
    OWNER,
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

SPEC = "docs/specs/journal.md v0.24 §9.12 (DEC-780)"
AT = "2026-09-21T15:00:00.000000000Z"
IDS = {
    "read": "01J8Z3R1A000000000000000R1",
    "export": "01J8Z3R2A000000000000000R2",
    "view": "01J8Z3R3A000000000000000R3",
    "verified": "01J8Z3R4A000000000000000R4",
    "failed": "01J8Z3R5A000000000000000R5",
}
ACCOUNT_STREAM = f"acct:{WORKSPACE}:01J8Z2ACCT00000000000000A1"
FOREIGN_STREAM = "acct:ws_01J8Z9:01J8Z9ACCT00000000000000A1"
GENESIS = "0" * 64
VERIFIER = "e" * 64
VIEW = "f" * 64
AUDITOR = {"kind": "user", "id": "user_auditor_01", "version": "1", "build": None}
# §3's client actor (DEC-671): an owner-connected agent, recorded with the user it acts for.
CLIENT = {"kind": "client", "id": "client_01", "version": "1", "build": None, "on_behalf_of": OWNER}
BROKER_ACTOR = {"kind": "broker", "id": "alpaca", "version": "v2", "build": None}
# The `PlatformOperatorAction` that opened a customer-approved break-glass window (§7).
BREAK_GLASS = "01J8Z3R9A000000000000000B1"
OPERATOR_ACTOR = {"kind": "platform_operator", "id": "operator_01", "version": "1", "build": None}
AGENT_ACTOR = {"kind": "agent", "id": "agent_a", "version": "0.1.0", "build": "sha256:" + "c" * 64}
# Every member name a §9.12 payload may carry, at any depth: streams, positions, hashes, check
# codes, and opaque IDs. An instrument, order, position, or mandate member is none of them.
VOCABULARY = frozenset(
    (
        "accessor",
        "operation",
        "ranges",
        "resources",
        "form",
        "verifier_digest",
        "view",
        "trigger",
        "result",
        "stream_id",
        "from_seq",
        "to_seq",
        "prev_hash",
        "to_hash",
        "failure",
        "check",
        "seq",
    )
)


def span(stream: str, first: int, last: int, prev: str, head: str | None) -> dict:
    return {"stream_id": stream, "from_seq": first, "to_seq": last, "prev_hash": prev, "to_hash": head}


def envelope(name: str, event_type: str, actor: dict, payload: dict, causation=None) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": IDS[name],
        "stream_id": STREAM,
        "event_type": event_type,
        "schema_version": 1,
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
    ranges = [
        span(ACCOUNT_STREAM, 1, 40, GENESIS, "1" * 64),
        span(AGENT_STREAM, 12, 30, "2" * 64, "3" * 64),
        span(STREAM, 1, 9, GENESIS, "4" * 64),
    ]
    read = {
        "accessor": AUDITOR["id"],
        "operation": "journal_events",
        "ranges": copy.deepcopy(ranges[1:2]),
        "resources": [],
        "result": None,
    }
    export = {"form": "canonical", "ranges": copy.deepcopy(ranges), "verifier_digest": VERIFIER, "view": None}
    view = {"form": "csv", "ranges": copy.deepcopy(ranges[:1]), "verifier_digest": VERIFIER, "view": VIEW}
    checked = [{**r, "failure": None} for r in copy.deepcopy(ranges)]
    failed = [
        {**span(ACCOUNT_STREAM, 1, 40, GENESIS, None), "failure": {"check": "rehash_mismatch", "seq": 17}},
        {**span(STREAM, 1, 9, GENESIS, "4" * 64), "failure": None},
    ]
    return {
        "records_read": envelope("read", "RecordsAccessed", AUDITOR, read),
        "export_canonical": envelope("export", "ExportCreated", AUDITOR, export),
        "export_view": envelope("view", "ExportCreated", AUDITOR, view, IDS["export"]),
        "verification_pass": envelope(
            "verified", "VerificationRun", SERVICES, {"trigger": "weekly", "ranges": checked, "result": "pass"}
        ),
        "verification_fail": envelope(
            "failed",
            "VerificationRun",
            USER,
            {"trigger": "request", "ranges": failed, "result": "fail"},
            IDS["export"],
        ),
    }


BASES = base_drafts()
RANGE_PATH = re.compile(r"payload\.ranges\[(\d+)\]\.(.+)")


def folded(base: str, changes: list[dict]) -> list[dict]:
    """`changes` in the vectors' form, which replaces whole payload members: every change inside a
    range (`payload.ranges[i].member`, or `...failure.member`) is folded into one replacement of the
    base draft's `ranges`, after the other changes."""
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
    """`changes`, then `artifact_refs` set to the changed payload's digests (§3), so a case that edits
    a reference member breaks only the rule it names."""
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
# Each member, a value of the wrong JSON kind (`schema`) and, where its type constrains a string, a
# string of the wrong form (`non_canonical`).
MEMBER_CASES = {
    "records_read": (
        ("payload.accessor", 7, ""),
        ("payload.operation", 7, "journal events"),
        ("payload.ranges", "all", None),
        ("payload.resources", "agent_a", None),
        ("payload.result", 7, "sha256:" + "A" * 64),
        (f"{R0}.stream_id", 7, "acct:only_one_segment"),
        (f"{R0}.from_seq", "12", None),
        (f"{R0}.to_seq", "30", None),
        (f"{R0}.prev_hash", 7, "2" * 63),
        (f"{R0}.to_hash", 7, "sha256:" + "3" * 64),
    ),
    "export_canonical": (
        ("payload.form", 7, "examination_bundle"),
        ("payload.verifier_digest", 7, "sha256:" + "e" * 64),
    ),
    "export_view": (("payload.view", 7, "F" * 64),),
    "verification_fail": (
        ("payload.trigger", 7, "hourly"),
        ("payload.result", True, "failed"),
        (f"{R0}.to_hash", 7, "4" * 64 + "\n"),
        (f"{R0}.failure", "rehash_mismatch", None),
        (f"{R0}.failure.check", 7, "chain_broken"),
        (f"{R0}.failure.seq", "17", None),
    ),
}
# Non-nullable members a `null` must not satisfy, each its own case.
NULLED = {
    "records_read": (
        "payload.accessor",
        "payload.operation",
        "payload.ranges",
        "payload.resources",
        f"{R0}.stream_id",
        f"{R0}.from_seq",
        f"{R0}.to_seq",
        f"{R0}.prev_hash",
        f"{R0}.to_hash",
    ),
    "export_canonical": ("payload.form", "payload.verifier_digest"),
    "verification_fail": ("payload.trigger", "payload.result", f"{R0}.failure.check"),
}


def seeded_key(path: str) -> str:
    """The `loose` and `nullable` seeded-bug key suffix of a member: its record and name."""
    depth = "failure" if ".failure." in path else "range" if "]." in path else "payload"
    return f"{depth}.{path.rsplit('.', 1)[-1]}"


def member_drafts() -> list[dict]:
    out = []
    for base, cases in MEMBER_CASES.items():
        for path, wrong_kind, wrong_form in cases:
            label = path.removeprefix("payload.")
            out.append(invalid(f"{base}.{label}.kind", "§9.12 types", base, [change(path, wrong_kind)], "schema", path))
            if wrong_form is not None:
                out.append(
                    invalid(f"{base}.{label}.form", "§9.12 types", base, [change(path, wrong_form)], "non_canonical", path)
                )
    for base, members in NULLED.items():
        for path in members:
            label = path.removeprefix("payload.")
            out.append(invalid(f"{base}.{label}.null", "§9.12 types", base, [change(path, None)], "schema", path))
    for base in BASES:
        first = next(iter(BASES[base]["payload"]))
        out.append(
            invalid(f"{base}.missing", "§9.12 closed", base, [delete(f"payload.{first}")], "schema", f"payload.{first}")
        )
        out.append(invalid(f"{base}.extra", "§9.12 closed", base, [change("payload.instrument", "x")], "schema", "payload.instrument"))
    out.append(
        invalid("range_missing_member", "§9.12 closed", "records_read", [delete(f"{R0}.to_hash")], "schema", f"{R0}.to_hash")
    )
    out.append(
        invalid("range_extra_member", "§9.12 closed", "records_read", [change(f"{R0}.quantity", "1")], "schema", f"{R0}.quantity")
    )
    out.append(
        invalid(
            "failure_extra_member",
            "§9.12 closed",
            "verification_fail",
            [change(f"{R0}.failure.detail", "x")],
            "schema",
            f"{R0}.failure.detail",
        )
    )
    return out


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule, or the rules its `also` lists; together they cover every
    §9.12 member type and rule."""
    read, export, view = "records_read", "export_canonical", "export_view"
    ok, bad = "verification_pass", "verification_fail"
    return [
        *member_drafts(),
        invalid(
            "read_wrong_stream",
            "§9.12: the records are control-stream records",
            read,
            [change("stream_id", AGENT_STREAM)],
            "wrong_stream",
            "event_type",
        ),
        invalid(
            "export_digest_listed_as_an_artifact",
            "§9.12 types: a `digest` names no stored artifact, so `artifact_refs` does not list it",
            view,
            [],
            "artifact_refs",
            "artifact_refs",
        )
        | {"changes": [change("artifact_refs", ["sha256:" + VERIFIER])]},
        invalid(
            "read_result_not_listed",
            "§3: `artifact_refs` lists the stored result",
            read,
            [],
            "artifact_refs",
            "artifact_refs",
        )
        | {"changes": [change("payload.result", "sha256:" + "9" * 64)]},
        invalid("read_nothing", "rule 96: at least one range", read, [change("payload.ranges", [])], "schema", "payload.ranges"),
        invalid(
            "read_another_workspace",
            "rule 96: tenant isolation (workspace API API-9)",
            read,
            [change(f"{R0}.stream_id", FOREIGN_STREAM)],
            "schema",
            f"{R0}.stream_id",
        ),
        invalid(
            "export_another_workspace_second",
            "rule 96: every range, not only the first",
            export,
            [change(f"{R1}.stream_id", "agent:ws_01J8Z9:agent_a")],
            "schema",
            f"{R1}.stream_id",
        ),
        invalid(
            "verify_another_workspace_control",
            "rule 96: the control stream of another workspace",
            ok,
            [change("payload.ranges[2].stream_id", "ctl:ws_01J8Z9")],
            "schema",
            "payload.ranges[2].stream_id",
        ),
        invalid(
            "range_from_seq_zero",
            "rule 96: seq starts at 1",
            export,
            [change(f"{R0}.from_seq", 0)],
            "schema",
            f"{R0}.from_seq",
            also=[("schema", f"{R0}.prev_hash")],
        ),
        invalid(
            "range_reversed",
            "rule 96: `to_seq` is at least `from_seq`",
            read,
            [change(f"{R0}.to_seq", 11)],
            "schema",
            f"{R0}.to_seq",
        ),
        invalid(
            "range_from_one_not_genesis",
            "rule 96: seq 1 follows 64 zeros",
            export,
            [change(f"{R0}.prev_hash", "9" * 64)],
            "schema",
            f"{R0}.prev_hash",
        ),
        invalid(
            "range_genesis_not_from_one",
            "rule 96: only seq 1 follows 64 zeros",
            read,
            [change(f"{R0}.prev_hash", GENESIS)],
            "schema",
            f"{R0}.prev_hash",
        ),
        invalid(
            "ranges_out_of_order",
            "rule 96: ranges sort by stream",
            export,
            [change(f"{R1}.stream_id", f"acct:{WORKSPACE}:01J8Z2ACCT00000000000000A0")],
            "schema",
            f"{R1}.stream_id",
        ),
        invalid(
            "ranges_overlap",
            "rule 96: two ranges of one stream never overlap",
            read,
            [change("payload.ranges", [span(AGENT_STREAM, 12, 30, "2" * 64, "3" * 64), span(AGENT_STREAM, 30, 31, "3" * 64, "5" * 64)])],
            "schema",
            f"{R1}.stream_id",
        ),
        invalid(
            "ranges_repeat",
            "rule 96: one range listed twice",
            read,
            [change("payload.ranges", [span(AGENT_STREAM, 12, 30, "2" * 64, "3" * 64)] * 2)],
            "schema",
            f"{R1}.stream_id",
        ),
        invalid(
            "read_accessor_not_the_reader",
            "rule 97: the accessor is the actor",
            read,
            [change("payload.accessor", OWNER)],
            "schema",
            "payload.accessor",
        ),
        invalid(
            "read_resource_not_an_id",
            "§9.12 types: a resource is an opaque `id`",
            read,
            [change("payload.resources", ["agent a"])],
            "non_canonical",
            "payload.resources[0]",
        ),
        invalid(
            "read_resources_unsorted",
            "rule 97: resources sort by bytes",
            read,
            [change("payload.resources", ["notice_01", "agent_a"])],
            "schema",
            "payload.resources",
        ),
        invalid(
            "read_resource_twice",
            "rule 97: each resource once",
            read,
            [change("payload.resources", ["agent_a", "agent_a"])],
            "schema",
            "payload.resources",
        ),
        invalid(
            "read_by_an_agent",
            "rule 97: an agent runtime reads no records outside the product views",
            read,
            [change("actor", AGENT_ACTOR), change("payload.accessor", AGENT_ACTOR["id"])],
            "schema",
            "actor.kind",
        ),
        invalid(
            "client_read_for_no_one",
            "§3 rule 81: a client actor names the user it acts for",
            read,
            [change("actor", {k: v for k, v in CLIENT.items() if k != "on_behalf_of"}), change("payload.accessor", CLIENT["id"])],
            "schema",
            "actor.on_behalf_of",
        ),
        invalid(
            "client_read_with_a_build",
            "§3 rule 82: a client's read uses the one client actor shape, external, with no build",
            read,
            [change("actor", {**CLIENT, "build": "sha256:" + "c" * 64}), change("payload.accessor", CLIENT["id"])],
            "schema",
            "actor.build",
        ),
        invalid(
            "client_read_recorded_as_its_human",
            "rule 97: a client is its own accessor, never the user it acts for",
            read,
            [change("actor", CLIENT), change("payload.accessor", OWNER)],
            "schema",
            "payload.accessor",
        ),
        invalid(
            "client_exports",
            "§3 rule 83: a client never exports (workspace API §3.8)",
            export,
            [change("actor", CLIENT)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "client_verifies",
            "§3 rule 83: a client never verifies",
            bad,
            [change("actor", CLIENT)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "read_by_a_broker",
            "rule 97: a broker reads no records",
            read,
            [change("actor", BROKER_ACTOR), change("payload.accessor", BROKER_ACTOR["id"])],
            "schema",
            "actor.kind",
        ),
        invalid(
            "operator_read_without_its_break_glass",
            "rule 97: platform staff read only under an approved break-glass action (§7)",
            read,
            [change("actor", OPERATOR_ACTOR), change("payload.accessor", OPERATOR_ACTOR["id"])],
            "schema",
            "causation_id",
        ),
        invalid(
            "export_by_a_broker",
            "rule 98: a user or a service account exports",
            export,
            [change("actor", BROKER_ACTOR)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "export_by_an_operator",
            "rule 98: platform staff do not export; a break-glass export fails closed (§9.12)",
            export,
            [change("actor", OPERATOR_ACTOR)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "scheduled_run_by_a_broker",
            "rule 99: a scheduled run is the verifier's own",
            ok,
            [change("actor", BROKER_ACTOR)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "requested_run_by_an_operator",
            "rule 99: platform staff do not run a verification; it fails closed (§9.12)",
            bad,
            [change("actor", OPERATOR_ACTOR)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "export_by_an_agent",
            "rule 98: a user or a service account exports",
            export,
            [change("actor", AGENT_ACTOR)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "canonical_with_a_view",
            "rule 98: a view exactly for a view form",
            export,
            [change("payload.view", VIEW)],
            "schema",
            "payload.view",
        ),
        invalid(
            "view_without_its_bytes",
            "rule 98",
            view,
            [change("payload.view", None)],
            "schema",
            "payload.view",
        ),
        invalid(
            "scheduled_run_by_a_user",
            "rule 99: a scheduled run is the verifier's own",
            ok,
            [change("actor", USER)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "requested_run_by_an_agent",
            "rule 99",
            bad,
            [change("actor", AGENT_ACTOR)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "event_check_without_its_seq",
            "rule 100: a per-event check names its seq",
            bad,
            [change(f"{R0}.failure.seq", None)],
            "schema",
            f"{R0}.failure.seq",
        ),
        invalid(
            "range_check_with_a_seq",
            "rule 100: a range check names no seq",
            bad,
            [change(f"{R0}.failure.check", "anchor_root_mismatch")],
            "schema",
            f"{R0}.failure.seq",
        ),
        invalid(
            "failure_after_the_range",
            "rule 100: the seq lies in the range",
            bad,
            [change(f"{R0}.failure.seq", 41)],
            "schema",
            f"{R0}.failure.seq",
        ),
        invalid(
            "failure_before_the_range",
            "rule 100: the seq lies in the range",
            bad,
            [change(f"{R0}.failure.seq", 0)],
            "schema",
            f"{R0}.failure.seq",
        ),
        invalid(
            "passed_range_without_its_head",
            "rule 100: a passed range names the head it verified",
            ok,
            [change(f"{R1}.to_hash", None)],
            "schema",
            f"{R1}.to_hash",
        ),
        invalid(
            "fail_reported_as_pass",
            "rule 101",
            bad,
            [change("payload.result", "pass")],
            "schema",
            "payload.result",
        ),
        invalid(
            "pass_reported_as_fail",
            "rule 101",
            ok,
            [change("payload.result", "fail")],
            "schema",
            "payload.result",
        ),
    ]


def valid_drafts() -> list[dict]:
    """Cases a rule might be misread to refuse; each must be accepted."""
    read, export, view = "records_read", "export_canonical", "export_view"
    ok, bad = "verification_pass", "verification_fail"
    return [
        valid(
            "read_one_event",
            "rule 96: a range of one event",
            read,
            [change(f"{R0}.to_seq", 12)],
        ),
        valid(
            "read_two_ranges_of_one_stream",
            "rule 96: disjoint ranges of one stream, adjacent included",
            read,
            [
                change(
                    "payload.ranges",
                    [span(AGENT_STREAM, 12, 30, "2" * 64, "3" * 64), span(AGENT_STREAM, 31, 31, "3" * 64, "5" * 64)],
                )
            ],
        ),
        valid(
            "read_by_a_service_account",
            "rule 97: a service account is a `system` actor (identity spec §3.1)",
            read,
            [change("actor", SERVICES), change("payload.accessor", SERVICES["id"])],
        ),
        valid(
            "client_reads_its_own_records",
            "rule 97 and §3 rule 83: a client records its own read, as its own accessor (workspace API §3.8)",
            read,
            [change("actor", CLIENT), change("payload.accessor", CLIENT["id"]), change("payload.operation", "agent_status")],
        ),
        valid(
            "read_by_an_operator",
            "rule 97 and §7: platform staff's break-glass reads are journaled where the customer reads them",
            read,
            [
                change("actor", OPERATOR_ACTOR),
                change("payload.accessor", OPERATOR_ACTOR["id"]),
                change("causation_id", BREAK_GLASS),
            ],
        ),
        valid(
            "read_scheduler_and_notice_streams",
            "§9.12 types: every stream type of §2",
            read,
            [
                change(
                    "payload.ranges",
                    [
                        span(f"clock:{WORKSPACE}", 1, 3, GENESIS, "6" * 64),
                        span(f"ntf:{WORKSPACE}", 4, 5, "7" * 64, "8" * 64),
                    ],
                )
            ],
        ),
        valid(
            "notice_resolved_with_resources_and_result",
            "rule 97: resources and a stored result (workspace API §3.9)",
            read,
            [
                change("payload.operation", "resolve_notice"),
                change("payload.ranges", [span(f"ntf:{WORKSPACE}", 4, 5, "7" * 64, "8" * 64)]),
                change("payload.resources", ["agent_a", "notice_01J8Z3"]),
                change("payload.result", "sha256:" + "9" * 64),
            ],
        ),
        valid("json_view", "rule 98", view, [change("payload.form", "json")]),
        valid("export_by_a_service_account", "rule 98", export, [change("actor", SERVICES)]),
        valid("requested_run_by_a_service_account", "rule 99", bad, [change("actor", SERVICES)]),
        valid("requested_run_passed", "rules 99 and 101", ok, [change("actor", USER), change("payload.trigger", "request")]),
        valid(
            "restore_drill_failed_on_a_range_check",
            "rules 100 and 101: a range check names no seq and may keep its head",
            bad,
            [
                change("actor", SERVICES),
                change("payload.trigger", "restore_drill"),
                change(f"{R0}.failure", {"check": "anchor_root_mismatch", "seq": None}),
                change(f"{R0}.to_hash", "1" * 64),
            ],
        ),
        valid(
            "failure_at_the_first_event",
            "rule 100: the range's own bounds are inside",
            bad,
            [change(f"{R0}.failure", {"check": "prev_hash_mismatch", "seq": 1})],
        ),
        valid(
            "failure_at_the_last_event",
            "rule 100",
            bad,
            [change(f"{R0}.failure", {"check": "seq_gap", "seq": 40})],
        ),
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = ("drafts.valid", "drafts.tenant", "drafts.vocabulary", "drafts.result", "invalid_drafts", "valid_drafts")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def workspace_of(stream: str) -> str:
    """§2's `{workspace_id}`: the text between the first and second colon, found by position rather
    than by rule 96's split."""
    start = stream.index(":") + 1
    end = stream.find(":", start)
    return stream[start:] if end < 0 else stream[start:end]


def member_names(value) -> set[str]:
    if isinstance(value, dict):
        return set(value) | {n for v in value.values() for n in member_names(v)}
    if isinstance(value, list):
        return {n for v in value for n in member_names(v)}
    return set()


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
        foreign = [r["stream_id"] for r in draft["payload"]["ranges"] if workspace_of(r["stream_id"]) != home]
        if foreign:
            problems.append(found("drafts.tenant", f"{name} names another workspace's streams {foreign}"))
        extra = member_names(draft["payload"]) - VOCABULARY
        if extra:
            problems.append(found("drafts.vocabulary", f"{name} carries {sorted(extra)}"))
        if draft["event_type"] == "VerificationRun":
            failures = sum(1 for r in draft["payload"]["ranges"] if r["failure"] is not None)
            if draft["payload"]["result"] != ("fail" if failures else "pass"):
                problems.append(found("drafts.result", f"{name}: {failures} failed ranges, result {draft['payload']['result']}"))
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
    "rule.96.empty",
    "rule.96.workspace",
    "rule.96.from_seq",
    "rule.96.to_seq",
    "rule.96.genesis",
    "rule.96.order",
    "boundary.rule_96_from_seq",
    "boundary.rule_96_overlap",
    "rule.97.accessor",
    "rule.97.actor",
    "rule.97.break_glass",
    "kinds.97.agent",
    "kinds.97.broker",
    "kinds.98.broker",
    "kinds.98.platform_operator",
    "kinds.99.broker",
    "kinds.99.platform_operator",
    "rule.97.resources",
    "rule.98.actor",
    "rule.98.view",
    "rule.99",
    "rule.100.seq",
    "rule.100.inside",
    "rule.100.to_hash",
    "rule.101",
    "types.stream_id",
    "types.digest",
    "record.extra",
    "record.missing",
    "artifact_refs",
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

    def foreign_base(s):
        s["drafts"]["records_read"]["payload"]["ranges"][0]["stream_id"] = FOREIGN_STREAM

    def last_ranges(s, name):
        return next(c for c in reversed(case(s, "valid_drafts", name)["changes"]) if c["path"] == "payload.ranges")

    def foreign_valid(s):
        last_ranges(s, "read_one_event")["value"][0]["stream_id"] = "agent:ws_01J8Z9:agent_a"

    def instrument_valid(s):
        case(s, "valid_drafts", "json_view")["changes"].append(change("payload.operation_notes", "AAPL"))

    def result_flipped(s):
        s["drafts"]["verification_fail"]["payload"]["result"] = "pass"

    return [
        ("a base draft breaks rule 98", "drafts.valid", mutated(lambda s: s["drafts"]["export_canonical"]["payload"].update(view=VIEW))),
        ("a base read names another workspace's stream", "drafts.tenant", mutated(foreign_base)),
        ("a valid read names another workspace's stream", "drafts.tenant", mutated(foreign_valid)),
        ("a valid view carries an instrument", "drafts.vocabulary", mutated(instrument_valid)),
        ("a failed run is reported as passed", "drafts.result", mutated(result_flipped)),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "read_another_workspace")["expect"].update(path="payload.ranges")),
        ),
        (
            "a valid draft breaks a rule",
            "valid_drafts",
            mutated(lambda s: last_ranges(s, "read_one_event")["value"][0].update(to_seq=11)),
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
            escaped.append(f"records-access validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"records-access check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"records-access vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
