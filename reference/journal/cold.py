"""Journal spec v0.22 §9.11's reference vectors (DEC-783): the control stream's `AnchorComputed` and
`SegmentExported`.

The schemas and rules 81 to 86 live in `control.py`, beside §9.2's to §9.10's, so one validator
judges every closed schema. This module builds the `cold_records` section: a base draft of each
record, an invalid draft for every member type and rule, and valid drafts for the cases a rule might
be misread to refuse. It checks the section with oracles of its own: the base anchor's root is the
v3 vectors' own `merkle.root` over the same leaves, every accepted anchor's root is recomputed by an
iterative walk that shares no code with rule 82, every accepted segment's manifest hash is
recomputed from `json.dumps` rather than the canonicalizer, and every stream named is parsed for its
workspace independently of rules 81 and 84. Every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
import hashlib
import json
import re

from common import apply_change, change, delete, digest_strings
from control import (
    AGENT_STREAM,
    SERVICES,
    STREAM,
    USER,
    WORKSPACE,
    check_of,
    draft_for,
    manifest_hash,
    merkle_root,
    reported,
    violations,
)
from control import (
    invalid as control_invalid,
)
from control import (
    valid as control_valid,
)

SPEC = "docs/specs/journal.md v0.22 §9.11 (DEC-783)"
AT = "2026-09-21T16:00:00.000000000Z"
IDS = {"anchor": "01J8Z3C1A000000000000000C1", "segment": "01J8Z3C2A000000000000000C2"}
ACCOUNT_STREAM = f"acct:{WORKSPACE}:01J8Z2ACCT00000000000000A1"
GENESIS = "0" * 64
TOKEN = "sha256:" + "a" * 64
LEAF_PATH = re.compile(r"payload\.leaves\[(\d+)\]\.(.+)")


def envelope(name: str, event_type: str, actor: dict, payload: dict) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": IDS[name],
        "stream_id": STREAM,
        "event_type": event_type,
        "schema_version": 1,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": None,
        "correlation_id": None,
        "actor": dict(actor),
        "config_refs": {},
        "payload": payload,
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
    }


def segment(stream: str, first: int, last: int, prev: str) -> dict:
    payload = {
        "stream_id": stream,
        "first_seq": first,
        "last_seq": last,
        "first_prev_hash": prev,
        "last_hash": "6" * 64,
        "file_sha256": "7" * 64,
    }
    return {**payload, "manifest_hash": manifest_hash(payload)}


def base_drafts(v3: dict) -> dict[str, dict]:
    leaves = [{k: leaf[k] for k in ("hash", "seq", "stream_id")} for leaf in v3["merkle"]["leaves"]]
    anchor = {"leaves": leaves, "root": merkle_root(leaves), "token": TOKEN}
    return {
        "anchor": envelope("anchor", "AnchorComputed", SERVICES, anchor),
        "segment": envelope("segment", "SegmentExported", SERVICES, segment(ACCOUNT_STREAM, 4, 9, "5" * 64)),
    }


class Section:
    """The base drafts, built once from the v3 vectors' own `merkle` leaves."""

    bases: dict[str, dict] = {}


def folded(base: str, changes: list[dict]) -> list[dict]:
    """`changes` in the vectors' form, which replaces whole payload members: every change inside a
    leaf (`payload.leaves[i].member`) is folded into one replacement of the base's `leaves`."""
    leaves = copy.deepcopy(Section.bases[base]["payload"].get("leaves"))
    out, inside = [], False
    for item in changes:
        found = LEAF_PATH.fullmatch(item["path"])
        if not found:
            out.append(item)
            continue
        apply_change(leaves[int(found.group(1))], {**item, "path": found.group(2)})
        inside = True
    return [*out, change("payload.leaves", leaves)] if inside else out


def refs_kept(base: str, changes: list[dict]) -> list[dict]:
    """`changes`, then `artifact_refs` set to the changed payload's digests (§3)."""
    changes = folded(base, changes)
    draft = copy.deepcopy(Section.bases[base])
    for item in changes:
        apply_change(draft, item)
    return [*changes, change("artifact_refs", sorted(digest_strings(draft["payload"])))]


def invalid(name, clause, base, changes, reason, path, also=()):
    return control_invalid(name, clause, base, refs_kept(base, changes), reason, path, also)


def valid(name, clause, base, changes):
    return control_valid(name, clause, base, refs_kept(base, changes))


L0 = "payload.leaves[0]"
L1 = "payload.leaves[1]"
# Each member, a value of the wrong JSON kind (`schema`) and, where its type constrains a string, a
# string of the wrong form (`non_canonical`).
MEMBER_CASES = {
    "anchor": (
        ("payload.leaves", "all", None),
        ("payload.root", 7, "sha256:" + "3" * 64),
        ("payload.token", 7, "a" * 64),
        (f"{L0}.hash", 7, "2" * 63),
        (f"{L0}.seq", "5", None),
        (f"{L0}.stream_id", 7, "acct:only_one_segment"),
    ),
    "segment": (
        ("payload.stream_id", 7, "ctl:ws_01J8Z2:extra"),
        ("payload.first_seq", "4", None),
        ("payload.last_seq", "9", None),
        ("payload.first_prev_hash", 7, "5" * 64 + "\n"),
        ("payload.last_hash", 7, "6" * 65),
        ("payload.file_sha256", 7, "7" * 63 + "G"),
        ("payload.manifest_hash", 7, "sha256:" + "8" * 64),
    ),
}
NULLED = {
    "anchor": ("payload.leaves", "payload.root", f"{L0}.hash", f"{L0}.seq", f"{L0}.stream_id"),
    "segment": (
        "payload.stream_id",
        "payload.first_seq",
        "payload.last_seq",
        "payload.first_prev_hash",
        "payload.last_hash",
        "payload.file_sha256",
        "payload.manifest_hash",
    ),
}


def seeded_key(path: str) -> str:
    depth = "leaf" if "]." in path else "payload"
    return f"{depth}.{path.rsplit('.', 1)[-1]}"


def member_drafts() -> list[dict]:
    out = []
    for base, cases in MEMBER_CASES.items():
        for path, wrong_kind, wrong_form in cases:
            label = path.removeprefix("payload.")
            out.append(invalid(f"{base}.{label}.kind", "§9.11 types", base, [change(path, wrong_kind)], "schema", path))
            if wrong_form is not None:
                out.append(
                    invalid(f"{base}.{label}.form", "§9.11 types", base, [change(path, wrong_form)], "non_canonical", path)
                )
    for base, members in NULLED.items():
        for path in members:
            label = path.removeprefix("payload.")
            out.append(invalid(f"{base}.{label}.null", "§9.11 types", base, [change(path, None)], "schema", path))
    for base in ("anchor", "segment"):
        first = next(iter(Section.bases[base]["payload"]))
        out.append(invalid(f"{base}.missing", "§9.11 closed", base, [delete(f"payload.{first}")], "schema", f"payload.{first}"))
        out.append(invalid(f"{base}.extra", "§9.11 closed", base, [change("payload.instrument", "x")], "schema", "payload.instrument"))
    out.append(invalid("leaf_missing_member", "§9.11 closed", "anchor", [delete(f"{L0}.hash")], "schema", f"{L0}.hash"))
    out.append(invalid("leaf_extra_member", "§9.11 closed", "anchor", [change(f"{L0}.price", "1")], "schema", f"{L0}.price"))
    return out


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule, or the rules its `also` lists."""
    leaves = Section.bases["anchor"]["payload"]["leaves"]
    swapped = [leaves[1], leaves[0], leaves[2]]
    foreign = "acct:ws_01J8Z9:01J8Z9ACCT00000000000000A1"
    return [
        *member_drafts(),
        invalid("anchor_wrong_stream", "§9.11: an anchor is a control-stream record", "anchor", [change("stream_id", AGENT_STREAM)], "wrong_stream", "event_type"),
        invalid("token_not_listed", "§3: `artifact_refs` lists the token", "anchor", [], "artifact_refs", "artifact_refs")
        | {"changes": [change("artifact_refs", [])]},
        invalid("anchor_of_nothing", "rule 81: at least one leaf", "anchor", [change("payload.leaves", []), change("payload.root", "0" * 64)], "schema", "payload.leaves"),
        invalid(
            "anchor_leaf_of_another_workspace",
            "rule 81: tenant isolation",
            "anchor",
            [change(f"{L0}.stream_id", foreign)],
            "schema",
            f"{L0}.stream_id",
            also=[("schema", "payload.root")],
        ),
        invalid("anchor_leaf_at_seq_zero", "rule 81: seq starts at 1", "anchor", [change(f"{L0}.seq", 0)], "schema", f"{L0}.seq", also=[("schema", "payload.root")]),
        invalid(
            "anchor_leaves_unsorted",
            "rule 81: leaves sort by stream",
            "anchor",
            [change("payload.leaves", swapped), change("payload.root", merkle_root(swapped))],
            "schema",
            f"{L1}.stream_id",
        ),
        invalid(
            "anchor_stream_twice",
            "rule 81: one leaf per stream",
            "anchor",
            [change("payload.leaves", [leaves[0], leaves[0]]), change("payload.root", merkle_root([leaves[0], leaves[0]]))],
            "schema",
            f"{L1}.stream_id",
        ),
        invalid("anchor_root_lies", "rule 82: the root is recomputed", "anchor", [change("payload.root", "3" * 64)], "schema", "payload.root"),
        invalid(
            "anchor_root_of_other_leaves",
            "rule 82: a root over the leaves before a change proves nothing",
            "anchor",
            [change(f"{L0}.hash", "1" * 64)],
            "schema",
            "payload.root",
        ),
        invalid("anchor_by_a_user", "rule 83", "anchor", [change("actor", USER)], "schema", "actor.kind"),
        invalid(
            "segment_of_another_workspace",
            "rule 84: tenant isolation",
            "segment",
            [change("payload.stream_id", foreign), change("payload.manifest_hash", manifest_hash({**Section.bases["segment"]["payload"], "stream_id": foreign}))],
            "schema",
            "payload.stream_id",
        ),
        invalid(
            "segment_from_seq_zero",
            "rule 84: seq starts at 1",
            "segment",
            [change("payload.first_seq", 0), change("payload.manifest_hash", manifest_hash({**Section.bases["segment"]["payload"], "first_seq": 0}))],
            "schema",
            "payload.first_seq",
        ),
        invalid(
            "segment_reversed",
            "rule 84: `last_seq` is at least `first_seq`",
            "segment",
            [change("payload.last_seq", 3), change("payload.manifest_hash", manifest_hash({**Section.bases["segment"]["payload"], "last_seq": 3}))],
            "schema",
            "payload.last_seq",
        ),
        invalid(
            "segment_from_one_not_genesis",
            "rule 84: seq 1 follows 64 zeros",
            "segment",
            [change("payload.first_seq", 1), change("payload.manifest_hash", manifest_hash({**Section.bases["segment"]["payload"], "first_seq": 1}))],
            "schema",
            "payload.first_prev_hash",
        ),
        invalid(
            "segment_genesis_not_from_one",
            "rule 84: only seq 1 follows 64 zeros",
            "segment",
            [change("payload.first_prev_hash", GENESIS), change("payload.manifest_hash", manifest_hash({**Section.bases["segment"]["payload"], "first_prev_hash": GENESIS}))],
            "schema",
            "payload.first_prev_hash",
        ),
        invalid(
            "segment_manifest_hash_of_another_manifest",
            "rule 85: the record and its manifest agree",
            "segment",
            [change("payload.last_hash", "9" * 64)],
            "schema",
            "payload.manifest_hash",
        ),
        invalid(
            "segment_manifest_hash_of_the_file",
            "rule 85: the manifest's hash, not the file's (DEC-263 item 3)",
            "segment",
            [change("payload.manifest_hash", "7" * 64)],
            "schema",
            "payload.manifest_hash",
        ),
        invalid("segment_by_a_user", "rule 86", "segment", [change("actor", USER)], "schema", "actor.kind"),
    ]


def valid_drafts() -> list[dict]:
    """Cases a rule might be misread to refuse; each must be accepted."""
    leaves = Section.bases["anchor"]["payload"]["leaves"]
    five = [{"hash": f"{i}" * 64, "seq": i + 1, "stream_id": f"acct:{WORKSPACE}:A{i}"} for i in range(5)]
    one = [leaves[0]]
    return [
        valid("anchor_of_one_stream", "rule 82: one leaf is its own root", "anchor", [change("payload.leaves", one), change("payload.root", merkle_root(one))]),
        valid("anchor_of_five_streams", "rule 82: the split is the largest power of two below the count", "anchor", [change("payload.leaves", five), change("payload.root", merkle_root(five))]),
        valid("anchor_without_its_token", "§9.11: a timestamping outage leaves the token null (§10)", "anchor", [change("payload.token", None)]),
        valid(
            "segment_from_genesis",
            "rule 84: a first segment follows 64 zeros",
            "segment",
            [change("payload", segment(ACCOUNT_STREAM, 1, 9, GENESIS))],
        ),
        valid(
            "segment_of_one_event",
            "rule 84: `first_seq` may equal `last_seq`",
            "segment",
            [change("payload", segment(f"ctl:{WORKSPACE}", 4, 4, "5" * 64))],
        ),
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = (
    "drafts.valid",
    "anchors.v3_root",
    "anchors.root",
    "segments.manifest",
    "drafts.tenant",
    "invalid_drafts",
    "valid_drafts",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def root_by_levels(leaves: list[dict]) -> str:
    """§10's tree built level by level, bottom up: at each level, the largest power-of-two prefix of
    a node's span pairs off first. It shares no code with rule 82's recursive split."""

    def leaf_hash(leaf: dict) -> bytes:
        body = json.dumps({k: leaf[k] for k in sorted(leaf)}, separators=(",", ":"), ensure_ascii=False)
        return hashlib.sha256(b"\x00" + body.encode()).digest()

    def span_root(lo: int, hi: int) -> bytes:
        n = hi - lo
        if n == 1:
            return hashes[lo]
        k = 1 << ((n - 1).bit_length() - 1)
        return hashlib.sha256(b"\x01" + span_root(lo, lo + k) + span_root(lo + k, hi)).digest()

    hashes = [leaf_hash(leaf) for leaf in leaves]
    return span_root(0, len(hashes)).hex()


def manifest_by_json(p: dict) -> str:
    manifest = {
        "file_sha256": p["file_sha256"],
        "first_prev_hash": p["first_prev_hash"],
        "first_seq": p["first_seq"],
        "last_hash": p["last_hash"],
        "last_seq": p["last_seq"],
        "stream": p["stream_id"],
    }
    return hashlib.sha256(json.dumps(manifest, separators=(",", ":")).encode()).hexdigest()


def workspace_of(stream: str) -> str:
    start = stream.index(":") + 1
    end = stream.find(":", start)
    return stream[start:] if end < 0 else stream[start:end]


def check_section(section: dict, v3: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    if section["drafts"]["anchor"]["payload"]["root"] != v3["merkle"]["root"]:
        problems.append(found("anchors.v3_root", "the base anchor's root is not the v3 vectors' merkle root"))
    accepted = [(n, copy.deepcopy(d)) for n, d in section["drafts"].items()]
    accepted += [(c["name"], draft_for(section, c)) for c in section["valid_drafts"]]
    for name, draft in accepted:
        p = draft["payload"]
        home = workspace_of(draft["stream_id"])
        if draft["event_type"] == "AnchorComputed":
            if p["root"] != root_by_levels(p["leaves"]):
                problems.append(found("anchors.root", f"{name}: root {p['root']}"))
            streams = [leaf["stream_id"] for leaf in p["leaves"]]
        else:
            if p["manifest_hash"] != manifest_by_json(p):
                problems.append(found("segments.manifest", f"{name}: manifest hash {p['manifest_hash']}"))
            streams = [p["stream_id"]]
        if any(workspace_of(s) != home for s in streams):
            problems.append(found("drafts.tenant", f"{name} names another workspace's stream"))
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
    "rule.81.empty",
    "rule.81.workspace",
    "rule.81.seq",
    "rule.81.order",
    "rule.82",
    "rule.83",
    "rule.84.workspace",
    "rule.84.first_seq",
    "rule.84.last_seq",
    "rule.84.genesis",
    "rule.85",
    "rule.86",
    "boundary.leaf_prefix",
    "boundary.merkle_split",
    "boundary.rule_84_first_seq",
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

    def leaves_and_root(s, name):
        changes = case(s, "valid_drafts", name)["changes"]
        leaves = next(c for c in changes if c["path"] == "payload.leaves")
        root = next(c for c in changes if c["path"] == "payload.root")
        return leaves, root

    def five_split_halved(s):
        leaves, root = leaves_and_root(s, "anchor_of_five_streams")
        root["value"] = merkle_root(leaves["value"], frozenset(["boundary.merkle_split"]))

    def base_root_of_other_leaves(s):
        anchor = s["drafts"]["anchor"]["payload"]
        anchor["leaves"][2]["hash"] = "1" * 64
        anchor["root"] = merkle_root(anchor["leaves"])

    def foreign_segment(s):
        payload = case(s, "valid_drafts", "segment_of_one_event")["changes"][0]["value"]
        payload["stream_id"] = "ctl:ws_01J8Z9"
        payload["manifest_hash"] = manifest_hash(payload)

    def manifest_of_the_file(s):
        payload = case(s, "valid_drafts", "segment_from_genesis")["changes"][0]["value"]
        payload["manifest_hash"] = payload["file_sha256"]

    return [
        ("a base draft breaks rule 86", "drafts.valid", mutated(lambda s: s["drafts"]["segment"]["actor"].update(kind="user", build=None))),
        ("the base anchor is rebuilt over other leaves", "anchors.v3_root", mutated(base_root_of_other_leaves)),
        ("a valid anchor's root is split at the half while rule 82 splits the same way", "anchors.root", mutated(five_split_halved)),
        ("a valid segment's manifest hash is the file's", "segments.manifest", mutated(manifest_of_the_file)),
        ("a valid segment names another workspace's stream", "drafts.tenant", mutated(foreign_segment)),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "anchor_root_lies")["expect"].update(path="payload.leaves")),
        ),
        (
            "a valid draft breaks a rule",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "anchor_without_its_token")["changes"].append(change("actor", USER))),
        ),
    ]


def run_mutants(section: dict, v3: dict) -> list[str]:
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
            escaped.append(f"cold-records validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"cold-records check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated, v3)}
        if check not in caught_by:
            escaped.append(f"cold-records vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section(v3: dict) -> dict:
    Section.bases = base_drafts(v3)
    return {
        "spec": SPEC,
        "drafts": copy.deepcopy(Section.bases),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
