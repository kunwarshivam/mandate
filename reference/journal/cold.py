"""Journal spec v0.27 §9.14's reference vectors (DEC-783): the control stream's `AnchorComputed` and
`SegmentExported`.

The schemas and rules 113 to 118 live in `control.py`, beside §9.2's to §9.13's, so one validator
judges every closed schema. This module builds the `cold_records` section: a base draft of each
record, an invalid draft for every member type and rule, and valid drafts for the cases a rule might
be misread to refuse. It checks the section with oracles of its own: the base anchor's root is the
v3 vectors' own `merkle.root` over the same leaves, every accepted anchor's root is recomputed by an
iterative walk that shares no code with rule 114, every accepted segment's manifest hash is
recomputed from `json.dumps` rather than the canonicalizer, and every stream named is parsed for its
workspace independently of rules 113 and 116. Every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
import hashlib
import json
import re

from common import apply_change, change, delete, digest_strings, hash_chain
from control import (
    AGENT_STREAM,
    SERVICES,
    STREAM,
    USER,
    WORKSPACE,
    check_of,
    draft_for,
    anchor_self_failure,
    manifest_hash,
    merkle_root,
    reported,
    trusted_start,
    violations,
)
from control import (
    invalid as control_invalid,
)
from control import (
    valid as control_valid,
)

SPEC = "docs/specs/journal.md v0.27 §9.14 (DEC-783)"
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


def invalid(name, clause, base, changes, reason, path, also=(), refs=None):
    """A refused draft. `artifact_refs` follows the changed payload's digests unless `refs` names the
    list to write instead, for the cases about `artifact_refs` itself."""
    kept = refs_kept(base, changes) if refs is None else [*folded(base, changes), change("artifact_refs", refs)]
    return control_invalid(name, clause, base, kept, reason, path, also)


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
            out.append(invalid(f"{base}.{label}.kind", "§9.14 types", base, [change(path, wrong_kind)], "schema", path))
            if wrong_form is not None:
                out.append(
                    invalid(f"{base}.{label}.form", "§9.14 types", base, [change(path, wrong_form)], "non_canonical", path)
                )
    for base, members in NULLED.items():
        for path in members:
            label = path.removeprefix("payload.")
            out.append(invalid(f"{base}.{label}.null", "§9.14 types", base, [change(path, None)], "schema", path))
    for base in ("anchor", "segment"):
        first = next(iter(Section.bases[base]["payload"]))
        out.append(invalid(f"{base}.missing", "§9.14 closed", base, [delete(f"payload.{first}")], "schema", f"payload.{first}"))
        out.append(invalid(f"{base}.extra", "§9.14 closed", base, [change("payload.instrument", "x")], "schema", "payload.instrument"))
    out.append(invalid("leaf_missing_member", "§9.14 closed", "anchor", [delete(f"{L0}.hash")], "schema", f"{L0}.hash"))
    out.append(invalid("leaf_extra_member", "§9.14 closed", "anchor", [change(f"{L0}.price", "1")], "schema", f"{L0}.price"))
    return out


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule, or the rules its `also` lists."""
    leaves = Section.bases["anchor"]["payload"]["leaves"]
    swapped = [leaves[1], leaves[0], leaves[2]]
    foreign = "acct:ws_01J8Z9:01J8Z9ACCT00000000000000A1"
    return [
        *member_drafts(),
        invalid("anchor_wrong_stream", "§9.14: an anchor is a control-stream record", "anchor", [change("stream_id", AGENT_STREAM)], "wrong_stream", "event_type"),
        invalid("token_not_listed", "§3: `artifact_refs` lists the token", "anchor", [], "artifact_refs", "artifact_refs", refs=[]),
        invalid(
            "anchor_without_its_own_leaf",
            "rule 113: an anchor names its own control stream's head",
            "anchor",
            [change("payload.leaves", leaves[:2]), change("payload.root", merkle_root(leaves[:2]))],
            "schema",
            "payload.leaves",
        ),
        invalid("anchor_of_nothing", "rule 113: at least one leaf", "anchor", [change("payload.leaves", []), change("payload.root", "0" * 64)], "schema", "payload.leaves"),
        invalid(
            "anchor_leaf_of_another_workspace",
            "rule 113: tenant isolation",
            "anchor",
            [change(f"{L0}.stream_id", foreign)],
            "schema",
            f"{L0}.stream_id",
            also=[("schema", "payload.root")],
        ),
        invalid("anchor_leaf_at_seq_zero", "rule 113: seq starts at 1", "anchor", [change(f"{L0}.seq", 0)], "schema", f"{L0}.seq", also=[("schema", "payload.root")]),
        invalid(
            "anchor_leaves_unsorted",
            "rule 113: leaves sort by stream",
            "anchor",
            [change("payload.leaves", swapped), change("payload.root", merkle_root(swapped))],
            "schema",
            f"{L1}.stream_id",
        ),
        invalid(
            "anchor_stream_twice",
            "rule 113: one leaf per stream",
            "anchor",
            [change("payload.leaves", [leaves[2], leaves[2]]), change("payload.root", merkle_root([leaves[2], leaves[2]]))],
            "schema",
            f"{L1}.stream_id",
        ),
        invalid("anchor_root_lies", "rule 114: the root is recomputed", "anchor", [change("payload.root", "3" * 64)], "schema", "payload.root"),
        invalid(
            "anchor_root_of_other_leaves",
            "rule 114: a root over the leaves before a change proves nothing",
            "anchor",
            [change(f"{L0}.hash", "1" * 64)],
            "schema",
            "payload.root",
        ),
        invalid("anchor_by_a_user", "rule 115", "anchor", [change("actor", USER)], "schema", "actor.kind"),
        invalid(
            "segment_of_another_workspace",
            "rule 116: tenant isolation",
            "segment",
            [change("payload.stream_id", foreign), change("payload.manifest_hash", manifest_hash({**Section.bases["segment"]["payload"], "stream_id": foreign}))],
            "schema",
            "payload.stream_id",
        ),
        invalid(
            "segment_from_seq_zero",
            "rule 116: seq starts at 1",
            "segment",
            [change("payload.first_seq", 0), change("payload.manifest_hash", manifest_hash({**Section.bases["segment"]["payload"], "first_seq": 0}))],
            "schema",
            "payload.first_seq",
        ),
        invalid(
            "segment_reversed",
            "rule 116: `last_seq` is at least `first_seq`",
            "segment",
            [change("payload.last_seq", 3), change("payload.manifest_hash", manifest_hash({**Section.bases["segment"]["payload"], "last_seq": 3}))],
            "schema",
            "payload.last_seq",
        ),
        invalid(
            "segment_from_one_not_genesis",
            "rule 116: seq 1 follows 64 zeros",
            "segment",
            [change("payload.first_seq", 1), change("payload.manifest_hash", manifest_hash({**Section.bases["segment"]["payload"], "first_seq": 1}))],
            "schema",
            "payload.first_prev_hash",
        ),
        invalid(
            "segment_genesis_not_from_one",
            "rule 116: only seq 1 follows 64 zeros",
            "segment",
            [change("payload.first_prev_hash", GENESIS), change("payload.manifest_hash", manifest_hash({**Section.bases["segment"]["payload"], "first_prev_hash": GENESIS}))],
            "schema",
            "payload.first_prev_hash",
        ),
        invalid(
            "segment_manifest_hash_of_another_manifest",
            "rule 117: the record and its manifest agree",
            "segment",
            [change("payload.last_hash", "9" * 64)],
            "schema",
            "payload.manifest_hash",
        ),
        invalid(
            "segment_manifest_hash_of_the_file",
            "rule 117: the manifest's hash, not the file's (DEC-263 item 3)",
            "segment",
            [change("payload.manifest_hash", "7" * 64)],
            "schema",
            "payload.manifest_hash",
        ),
        invalid("segment_by_a_user", "rule 118", "segment", [change("actor", USER)], "schema", "actor.kind"),
    ]


def valid_drafts() -> list[dict]:
    """Cases a rule might be misread to refuse; each must be accepted."""
    leaves = Section.bases["anchor"]["payload"]["leaves"]
    five = [{"hash": f"{i}" * 64, "seq": i + 1, "stream_id": f"acct:{WORKSPACE}:A{i}"} for i in range(4)]
    five.append({"hash": "4" * 64, "seq": 5, "stream_id": STREAM})
    one = [leaves[2]]
    return [
        valid("anchor_of_one_stream", "rule 114: one leaf, its own stream's, is its own root", "anchor", [change("payload.leaves", one), change("payload.root", merkle_root(one))]),
        valid("anchor_of_five_streams", "rule 114: the split is the largest power of two below the count", "anchor", [change("payload.leaves", five), change("payload.root", merkle_root(five))]),
        valid("anchor_without_its_token", "§9.14: a timestamping outage leaves the token null (§10)", "anchor", [change("payload.token", None)]),
        valid(
            "segment_from_genesis",
            "rule 116: a first segment follows 64 zeros",
            "segment",
            [change("payload", segment(ACCOUNT_STREAM, 1, 9, GENESIS))],
        ),
        valid(
            "segment_of_one_event",
            "rule 116: `first_seq` may equal `last_seq`",
            "segment",
            [change("payload", segment(f"ctl:{WORKSPACE}", 4, 4, "5" * 64))],
        ),
    ]


# --------------------------------------------------------------------------- §11 and the trusted start


def chained(event_type: str, seq: int, payload: dict) -> dict:
    """A control-stream body at `seq`, before `hash_chain` gives it its `prev_hash`."""
    body = envelope("anchor", event_type, SERVICES, payload)
    body["event_id"] = f"01J8Z3C3A00000000000000{seq:03d}"
    body["seq"] = seq
    body["recorded_at"] = AT
    return body


def range_case(name: str, clause: str, own: str, expect_failure: bool) -> dict:
    """A control stream of four events whose fourth is an anchor. `own` says what the anchor's leaf
    for its own stream holds: the head before it, a wrong `seq`, a wrong `hash`, or no leaf."""
    head = [
        chained("StreamOpened", 1, {"stream_type": "control", "workspace_id": WORKSPACE}),
        chained("SegmentExported", 2, segment(ACCOUNT_STREAM, 1, 3, GENESIS)),
        chained("SegmentExported", 3, segment(ACCOUNT_STREAM, 4, 9, "5" * 64)),
    ]
    first = hash_chain(copy.deepcopy(head), GENESIS)
    leaf = {"hash": first[2]["hash"], "seq": 3, "stream_id": STREAM}
    if own == "seq":
        leaf["seq"] = 2
    elif own == "hash":
        leaf["hash"] = first[1]["hash"]
    leaves = [{"hash": "1" * 64, "seq": 9, "stream_id": ACCOUNT_STREAM}]
    if own != "missing":
        leaves.append(leaf)
    anchor = chained("AnchorComputed", 4, {"leaves": leaves, "root": merkle_root(leaves), "token": TOKEN})
    entries = hash_chain([*head, anchor], GENESIS)
    expect = {"seq": 4, "check": "anchor_self_mismatch"} if expect_failure else None
    chain = [{"body": e["body"], "hash": e["hash"]} for e in entries]
    return {"name": name, "clause": clause, "chain": chain, "expect": expect}


def range_checks() -> list[dict]:
    return [
        range_case("anchor_names_the_head_before_it", "§11 anchor_self_mismatch: passes", "head", False),
        range_case("anchor_names_an_earlier_seq", "§11 anchor_self_mismatch: the seq before the anchor", "seq", True),
        range_case("anchor_names_another_events_hash", "§11 anchor_self_mismatch: that event's hash", "hash", True),
        range_case("anchor_leaves_its_own_stream_out", "§11 anchor_self_mismatch: a leaf for its own stream", "missing", True),
    ]


SEGMENT_START = segment(ACCOUNT_STREAM, 4, 9, "5" * 64)
FOREIGN_SEGMENT = segment(ACCOUNT_STREAM, 20, 30, "9" * 64)
ANCHOR_IDS = {"stamped": "01J8Z3C4A000000000000000S1", "unstamped": "01J8Z3C4A000000000000000U1"}


def start_records() -> list[dict]:
    """The records a resolver reads: one workspace's control stream, and a record of another
    workspace's that names this workspace's account stream."""

    def anchor(leaves: list[dict], token) -> dict:
        return {"leaves": leaves, "root": merkle_root(leaves), "token": token}

    stamped = [
        {"hash": "1" * 64, "seq": 9, "stream_id": ACCOUNT_STREAM},
        {"hash": "2" * 64, "seq": 3, "stream_id": STREAM},
    ]
    unstamped = [
        {"hash": "3" * 64, "seq": 12, "stream_id": ACCOUNT_STREAM},
        {"hash": "4" * 64, "seq": 5, "stream_id": STREAM},
    ]
    return [
        {"event_id": "01J8Z3C4A000000000000000G1", "event_type": "SegmentExported", "stream_id": STREAM, "payload": SEGMENT_START},
        {"event_id": ANCHOR_IDS["stamped"], "event_type": "AnchorComputed", "stream_id": STREAM, "payload": anchor(stamped, TOKEN)},
        {"event_id": ANCHOR_IDS["unstamped"], "event_type": "AnchorComputed", "stream_id": STREAM, "payload": anchor(unstamped, None)},
        {"event_id": "01J8Z3C4A000000000000000F1", "event_type": "SegmentExported", "stream_id": "ctl:ws_01J8Z9", "payload": FOREIGN_SEGMENT},
    ]


def start_case(name, clause, stream, from_seq, request, expect):
    return {"name": name, "clause": clause, "stream_id": stream, "from_seq": from_seq, "request": request, "expect": expect}


def trusted_starts() -> dict:
    manifest = {"kind": "manifest", "manifest_hash": SEGMENT_START["manifest_hash"]}
    stamped = {"kind": "anchor", "anchor_event_id": ANCHOR_IDS["stamped"]}
    unstamped = {"kind": "anchor", "anchor_event_id": ANCHOR_IDS["unstamped"]}
    genesis = {"kind": "genesis"}
    acct = ACCOUNT_STREAM
    return {
        "records": start_records(),
        "cases": [
            start_case("genesis_at_seq_one", "§11: the genesis start", acct, 1, genesis, {"from_seq": 1, "prev_hash": GENESIS}),
            start_case("genesis_after_seq_one", "§11: genesis is seq 1 only", acct, 2, genesis, None),
            start_case("segment_at_its_first_seq", "§9.14: a SegmentExported's start", acct, 4, manifest, {"from_seq": 4, "prev_hash": "5" * 64}),
            start_case("segment_entered_inside", "§9.14: a segment starts at its first_seq", acct, 5, manifest, None),
            start_case("segment_of_another_stream", "§9.14: the segment's own stream", AGENT_STREAM, 4, manifest, None),
            start_case("manifest_not_recorded", "§9.14: an absent manifest", acct, 4, {"kind": "manifest", "manifest_hash": "8" * 64}, None),
            start_case("stamped_anchor_after_its_leaf", "§9.14: a stamped anchor's leaf at n - 1", acct, 10, stamped, {"from_seq": 10, "prev_hash": "1" * 64}),
            start_case("stamped_anchor_at_its_leaf", "§9.14: the leaf is the event before the start", acct, 9, stamped, None),
            start_case("unstamped_anchor_is_no_start", "§9.14, DEC-783 item 8: a null token vouches for nothing", acct, 13, unstamped, None),
            start_case("anchor_without_the_stream", "§9.14: the anchor has no leaf for the stream", AGENT_STREAM, 10, stamped, None),
            start_case("another_workspaces_segment", "§9.14, DEC-767: only the workspace's own records", acct, 20, {"kind": "manifest", "manifest_hash": FOREIGN_SEGMENT["manifest_hash"]}, None),
        ],
    }


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = (
    "drafts.valid",
    "anchors.v3_root",
    "anchors.root",
    "segments.manifest",
    "drafts.tenant",
    "invalid_drafts",
    "valid_drafts",
    "ranges.chain",
    "ranges.reference",
    "ranges.walk",
    "starts.reference",
    "starts.resolve",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def root_by_levels(leaves: list[dict]) -> str:
    """§10's tree, built top down by its own split: each span of more than one leaf splits at the
    largest power of two below its length, found from the length's bit count rather than rule 114's
    doubling loop, and the halves are hashed recursively. It shares no code with rule 114."""

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


def walk_own_leaf(chain: list[dict]) -> dict | None:
    """The oracle's own §11 walk: an anchor's leaves must contain exactly the triple of its own
    stream, the previous entry's `seq` and the previous entry's `hash`."""
    for before, entry in zip(chain, chain[1:]):
        body = entry["body"]
        if body["event_type"] == "AnchorComputed":
            need = (body["stream_id"], before["body"]["seq"], before["hash"])
            have = {(leaf["stream_id"], leaf["seq"], leaf["hash"]) for leaf in body["payload"]["leaves"]}
            if need not in have:
                return {"seq": body["seq"], "check": "anchor_self_mismatch"}
    return None


def chain_breaks(chain: list[dict]) -> list[int]:
    """The `seq`s whose body does not re-hash to its `hash` or does not link to the one before."""
    bad, prev = [], GENESIS
    for entry in chain:
        body = entry["body"]
        text = json.dumps(body, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        if hashlib.sha256(text.encode()).hexdigest() != entry["hash"] or body["prev_hash"] != prev:
            bad.append(body["seq"])
        prev = entry["hash"]
    return bad


def resolve_start(records: list[dict], case: dict) -> dict | None:
    """The oracle's own resolver, written from §9.14's paragraph: filter by workspace first, then by
    the kind of start, and only then by the fit."""
    stream, n, request = case["stream_id"], case["from_seq"], case["request"]
    mine = [r for r in records if workspace_of(r["stream_id"]) == workspace_of(stream)]
    if request["kind"] == "genesis":
        return {"from_seq": 1, "prev_hash": GENESIS} if n == 1 else None
    if request["kind"] == "manifest":
        hits = [
            r["payload"]
            for r in mine
            if r["event_type"] == "SegmentExported"
            and r["payload"]["manifest_hash"] == request["manifest_hash"]
            and r["payload"]["stream_id"] == stream
            and r["payload"]["first_seq"] == n
        ]
        return {"from_seq": n, "prev_hash": hits[0]["first_prev_hash"]} if hits else None
    anchors = [
        r["payload"]
        for r in mine
        if r["event_type"] == "AnchorComputed" and r["event_id"] == request["anchor_event_id"] and r["payload"]["token"]
    ]
    leaves = [leaf for a in anchors for leaf in a["leaves"] if (leaf["stream_id"], leaf["seq"]) == (stream, n - 1)]
    return {"from_seq": n, "prev_hash": leaves[0]["hash"]} if leaves else None


def entries_of(chain: list[dict]) -> list[dict]:
    return [{"seq": e["body"]["seq"], "hash": e["hash"], "body": e["body"]} for e in chain]


def check_ranges_and_starts(section: dict) -> list[str]:
    problems = []
    for case in section["range_checks"]:
        chain = case["chain"]
        if chain_breaks(chain):
            problems.append(found("ranges.chain", f"{case['name']}: entries {chain_breaks(chain)} do not chain"))
        if anchor_self_failure(entries_of(chain)) != case["expect"]:
            problems.append(found("ranges.reference", f"{case['name']}: expected {case['expect']}"))
        if walk_own_leaf(chain) != case["expect"]:
            problems.append(found("ranges.walk", f"{case['name']}: expected {case['expect']}"))
    starts = section["trusted_starts"]
    for case in starts["cases"]:
        got = trusted_start(starts["records"], case["stream_id"], case["from_seq"], case["request"])
        if got != case["expect"]:
            problems.append(found("starts.reference", f"{case['name']}: expected {case['expect']}, got {got}"))
        if resolve_start(starts["records"], case) != case["expect"]:
            problems.append(found("starts.resolve", f"{case['name']}: expected {case['expect']}"))
    return problems


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
    return problems + check_ranges_and_starts(section)


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.113.empty",
    "rule.113.workspace",
    "rule.113.seq",
    "rule.113.order",
    "rule.113.self",
    "rule.114",
    "rule.115",
    "rule.116.workspace",
    "rule.116.first_seq",
    "rule.116.last_seq",
    "rule.116.genesis",
    "rule.117",
    "rule.118",
    "boundary.leaf_prefix",
    "boundary.merkle_split",
    "boundary.rule_116_first_seq",
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
        ("a base draft breaks rule 118", "drafts.valid", mutated(lambda s: s["drafts"]["segment"]["actor"].update(kind="user", build=None))),
        ("the base anchor is rebuilt over other leaves", "anchors.v3_root", mutated(base_root_of_other_leaves)),
        ("a valid anchor's root is split at the half while rule 114 splits the same way", "anchors.root", mutated(five_split_halved)),
        ("a valid segment's manifest hash is the file's", "segments.manifest", mutated(manifest_of_the_file)),
        ("a valid segment names another workspace's stream", "drafts.tenant", mutated(foreign_segment)),
        (
            "a range case's chain is altered after it was hashed",
            "ranges.chain",
            mutated(lambda s: s["range_checks"][0]["chain"][1]["body"]["payload"].update(last_seq=4)),
        ),
        (
            "a passing range is expected to fail",
            "ranges.reference",
            mutated(lambda s: s["range_checks"][0].update(expect={"seq": 4, "check": "anchor_self_mismatch"})),
        ),
        (
            "an anchor naming another event's hash is expected to pass",
            "ranges.walk",
            mutated(lambda s: s["range_checks"][2].update(expect=None)),
        ),
        (
            "an unstamped anchor is expected to be a start",
            "starts.resolve",
            mutated(lambda s: case(s["trusted_starts"], "cases", "unstamped_anchor_is_no_start").update(
                expect={"from_seq": 13, "prev_hash": "3" * 64})),
        ),
        (
            "a segment's start is expected at another hash",
            "starts.reference",
            mutated(lambda s: case(s["trusted_starts"], "cases", "segment_at_its_first_seq").update(
                expect={"from_seq": 4, "prev_hash": "6" * 64})),
        ),
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


RANGE_MUTANTS = ("self.missing", "self.seq", "self.hash")
START_MUTANTS = ("start.genesis_seq", "start.first_seq", "start.anchor_seq", "start.null_token", "start.workspace")


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
    for mutant in RANGE_MUTANTS:
        skip = frozenset([mutant])
        if all(anchor_self_failure(entries_of(c["chain"]), skip) == c["expect"] for c in section["range_checks"]):
            escaped.append(f"cold-records range mutant {mutant}")
    starts = section["trusted_starts"]
    for mutant in START_MUTANTS:
        skip = frozenset([mutant])
        if all(
            trusted_start(starts["records"], c["stream_id"], c["from_seq"], c["request"], skip) == c["expect"]
            for c in starts["cases"]
        ):
            escaped.append(f"cold-records start mutant {mutant}")
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
        "range_checks": range_checks(),
        "trusted_starts": trusted_starts(),
    }
