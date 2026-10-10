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

import collections
import copy
import hashlib
import json
import re

from common import apply_change, canon, change, delete, digest_strings, hash_chain, sha256_hex
from control import (
    AGENT_STREAM,
    ROW_COLUMNS,
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
    start_from_rows,
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
    rows = start_rows()
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
        "rows": rows,
        "row_cases": row_cases(rows),
    }


FOREIGN_CTL = "ctl:ws_01J8Z9"
ROW_IDS = tuple(f"01J8Z3C6A000000000000000R{i}" for i in range(1, 6))
OPENED, FROM_GENESIS, SEGMENT, ANCHOR, FOREIGN = range(5)
COLD = {s: {"state": s} for s in ("absent", "unreadable")}
FORGED_PREV = "e" * 64


def row_of(body: dict) -> dict:
    """A stored row as the hot store holds it: §11 check 2's columns, the hash, and the body bytes."""
    text = canon(body)
    return {**{c: body[c] for c in ROW_COLUMNS}, "hash": sha256_hex(text.encode()), "body": text}


def start_rows() -> list[dict]:
    """The workspace's control stream (`StreamOpened`, two `SegmentExported`, a stamped anchor), then a
    row of another workspace's control stream whose segment names this workspace's account stream."""

    def at(i: int, event_type: str, seq: int, payload: dict) -> dict:
        body = chained(event_type, seq, payload)
        body["event_id"] = ROW_IDS[i]
        return body

    head = [
        at(OPENED, "StreamOpened", 1, {"stream_type": "control", "workspace_id": WORKSPACE}),
        at(FROM_GENESIS, "SegmentExported", 2, segment(ACCOUNT_STREAM, 1, 3, GENESIS)),
        at(SEGMENT, "SegmentExported", 3, SEGMENT_START),
    ]
    leaves = [
        {"hash": "1" * 64, "seq": 9, "stream_id": ACCOUNT_STREAM},
        {"hash": hash_chain(copy.deepcopy(head), GENESIS)[2]["hash"], "seq": 3, "stream_id": STREAM},
    ]
    anchor = at(ANCHOR, "AnchorComputed", 4, {"leaves": leaves, "root": merkle_root(leaves), "token": TOKEN})
    foreign = at(FOREIGN, "SegmentExported", 1, FOREIGN_SEGMENT)
    foreign["stream_id"] = FOREIGN_CTL
    own = hash_chain([*head, anchor], GENESIS) + hash_chain([foreign], GENESIS)
    return [row_of(e["body"]) for e in own]


def cold_read(p: dict, suffix: str = "") -> dict:
    six = ("stream_id", "first_seq", "last_seq", "first_prev_hash", "last_hash", "file_sha256")
    return {"state": "read", "bytes": canon({("stream" if k == "stream_id" else k): p[k] for k in six}) + suffix}


def edited(row: dict, edit, rehash: bool) -> dict:
    """The row's body edited: stored again (re-hashed, its columns from the new body) or forged in
    place (its stored hash and columns kept)."""
    body = json.loads(row["body"])
    edit(body)
    return row_of(body) if rehash else {**row, "body": canon(body)}


def retyped(row: dict, text: str) -> dict:
    """The row stored with other bytes for the same body, re-hashed over them."""
    return {**row, "body": text, "hash": sha256_hex(text.encode())}


def new_prev(manifest: bool):
    def edit(body: dict) -> None:
        p, kept = body["payload"], body["payload"]["manifest_hash"]
        p["first_prev_hash"] = FORGED_PREV
        p["manifest_hash"] = manifest_hash(p) if manifest else kept
    return edit


def forged_leaf(body: dict) -> None:
    body["payload"]["leaves"][0]["hash"] = FORGED_PREV
    body["payload"]["root"] = merkle_root(body["payload"]["leaves"])


def leaf_edited(body: dict) -> None:
    body["payload"]["leaves"][0]["hash"] = FORGED_PREV


def leaves_reversed(body: dict) -> None:
    body["payload"]["leaves"].reverse()
    body["payload"]["root"] = merkle_root(body["payload"]["leaves"])


def unstamped(body: dict) -> None:
    body["payload"]["token"] = None


def moved_to(stream: str):
    def edit(body: dict) -> None:
        body["stream_id"] = stream
    return edit


def leaf_set(i: int, **members):
    """The anchor's `i`th leaf given `members`, its root recomputed over the leaves in stream order."""
    def edit(body: dict) -> None:
        leaves = body["payload"]["leaves"]
        leaves[i:i + 1] = [{**(leaves[i] if i < len(leaves) else {}), **members}]
        leaves.sort(key=lambda leaf: leaf["stream_id"].encode())
        body["payload"]["root"] = merkle_root(leaves)
    return edit


def appended(row: dict, prev: str, event_id: str, **members) -> dict:
    """The row recorded again at seq 5, chained to `prev`, as `event_id` with `members` changed."""
    return row_of({**json.loads(row["body"]), "seq": 5, "prev_hash": prev, "event_id": event_id, **members})


def doubled(row: dict, prev: str, member: str) -> dict:
    """The row recorded again at seq 5, chained to `prev`, its body stored with `member` written
    twice at the head of its payload, the first time with another value, and re-hashed."""
    again = row_of({**json.loads(row["body"]), "seq": 5, "prev_hash": prev})
    return retyped(again, again["body"].replace('"payload":{', '"payload":{' + member + ",", 1))


def row_cases(rows: list[dict]) -> list[dict]:
    """Each case's `replace` swaps one row of `rows` before the resolver reads them, and its `cold` is
    what the cold store gives for the start's segment. The reference's answer is recorded, once it
    agrees with the outcome the case was built for."""
    acct, seg, foreign = ACCOUNT_STREAM, json.loads(rows[SEGMENT]["body"])["payload"], FOREIGN_SEGMENT
    genesis_seg = json.loads(rows[FROM_GENESIS]["body"])["payload"]
    by_hash = {"kind": "manifest", "manifest_hash": seg["manifest_hash"]}
    by_anchor = {"kind": "anchor", "anchor_event_id": ROW_IDS[ANCHOR]}
    moved = json.loads(edited(rows[SEGMENT], new_prev(True), True)["body"])["payload"]
    loose = json.loads(rows[ANCHOR]["body"])
    lost = segment(acct, 10, 12, "1" * 64)
    stray = row_of({**json.loads(rows[SEGMENT]["body"]), "stream_id": acct, "event_id": "01J8Z3C6A000000000000000R7", "payload": lost})
    twice = row_of({**json.loads(rows[SEGMENT]["body"]), "event_id": "01J8Z3C6A000000000000000R6", "seq": 5, "prev_hash": rows[ANCHOR]["hash"]})
    tip = rows[ANCHOR]["hash"]
    specs = [
        ("genesis_start_reads_no_record", "§11: the genesis start", acct, 1, {"kind": "genesis"}, None, COLD["unreadable"], "start"),
        ("manifest_start_from_genesis", "DEC-787 items 3, 5: a checked row, the cold manifest read", acct, 1,
         {"kind": "manifest", "manifest_hash": genesis_seg["manifest_hash"]}, None, cold_read(genesis_seg), "start"),
        ("manifest_start_cold_confirmed", "DEC-787 items 3, 5: a checked row, the cold manifest read", acct, 4, by_hash, None, cold_read(seg), "start"),
        ("anchor_start_needs_no_cold_store", "DEC-787 item 3: a checked stamped anchor", acct, 10, by_anchor, None, COLD["unreadable"], "start"),
        ("another_workspaces_row", "§9.14, DEC-767: its own columns name another workspace", acct, 20,
         {"kind": "manifest", "manifest_hash": foreign["manifest_hash"]}, None, cold_read(foreign), "no_start"),
        ("manifest_cold_object_absent", "DEC-787 item 5: only the hot store vouches", acct, 4, by_hash, None, COLD["absent"], "cold_unreadable"),
        ("manifest_cold_store_unreadable", "DEC-787 item 5: only the hot store vouches", acct, 4, by_hash, None, COLD["unreadable"], "cold_unreadable"),
        ("manifest_rewritten_in_the_hot_store", "DEC-894 item 1: the cold manifest disagrees", acct, 4,
         {"kind": "manifest", "manifest_hash": moved["manifest_hash"]}, (SEGMENT, edited(rows[SEGMENT], new_prev(True), True)),
         cold_read(seg), "cold_manifest_mismatch"),
        ("cold_bytes_not_the_manifest", "DEC-894 item 1: the cold bytes hash to another digest", acct, 4, by_hash, None, cold_read(seg, "\n"), "cold_manifest_mismatch"),
        ("manifest_row_not_canonical", "§11 check 1: a space inserted, re-hashed", acct, 4, by_hash,
         (SEGMENT, retyped(rows[SEGMENT], "{ " + rows[SEGMENT]["body"][1:])), cold_read(seg), "non_canonical"),
        ("anchor_row_keys_unsorted", "§11 check 1: keys out of order, re-hashed", acct, 10, by_anchor,
         (ANCHOR, retyped(rows[ANCHOR], json.dumps(dict(sorted(loose.items(), reverse=True)), separators=(",", ":")))),
         COLD["unreadable"], "non_canonical"),
        ("workspace_column_claims_this_workspace", "§11 check 2: another workspace's body under this workspace's column", acct, 20,
         {"kind": "manifest", "manifest_hash": foreign["manifest_hash"]}, (FOREIGN, {**rows[FOREIGN], "stream_id": STREAM}),
         cold_read(foreign), "column_mismatch"),
        ("workspace_column_names_another", "§11 check 2: this workspace's body under another's column", acct, 4, by_hash,
         (SEGMENT, {**rows[SEGMENT], "stream_id": FOREIGN_CTL}), cold_read(seg), "no_start"),
        ("anchor_event_id_column_differs", "§11 check 2: the event_id column", acct, 10,
         {"kind": "anchor", "anchor_event_id": ROW_IDS[OPENED]}, (ANCHOR, {**rows[ANCHOR], "event_id": ROW_IDS[OPENED]}),
         COLD["unreadable"], "column_mismatch"),
        ("manifest_seq_column_differs", "§11 check 2: the seq column", acct, 4, by_hash, (SEGMENT, {**rows[SEGMENT], "seq": 7}), cold_read(seg), "column_mismatch"),
        ("anchor_leaf_forged_under_its_old_hash", "§11 check 4: the body forged, the stored hash kept", acct, 10, by_anchor,
         (ANCHOR, edited(rows[ANCHOR], forged_leaf, False)), COLD["unreadable"], "rehash_mismatch"),
        ("manifest_forged_under_its_old_hash", "§11 check 4: the body forged, the stored hash kept", acct, 4,
         {"kind": "manifest", "manifest_hash": moved["manifest_hash"]}, (SEGMENT, edited(rows[SEGMENT], new_prev(True), False)),
         cold_read(seg), "rehash_mismatch"),
        ("segment_on_an_account_stream", "§9.14: the workspace's control-stream records only", acct, 10, {"kind": "manifest", "manifest_hash": lost["manifest_hash"]}, (FOREIGN, stray), cold_read(lost), "no_start"),
        ("segment_exported_twice", "DEC-893 item 7: the segment recorded again at seq 5, chained, in the foreign row's place", acct, 4, by_hash, (FOREIGN, twice), cold_read(seg), "ambiguous_start"),
        ("manifest_row_breaks_rule_117", "rule 117: the manifest hash is not its fields'", acct, 4, by_hash,
         (SEGMENT, edited(rows[SEGMENT], new_prev(False), True)), cold_read(seg), "rule_117"),
        ("segment_copied_with_a_duplicate_key", "DEC-895 item 1: a candidate row fails §11 check 1, the good row beside it", acct, 4, by_hash,
         (FOREIGN, doubled(rows[SEGMENT], tip, f'"first_prev_hash":"{FORGED_PREV}"')), cold_read(seg), "non_canonical"),
        ("anchor_copied_with_a_duplicate_key", "DEC-895 item 1: a candidate row fails §11 check 1, the good row beside it", acct, 10, by_anchor,
         (FOREIGN, doubled(rows[ANCHOR], tip, '"token":null')), COLD["unreadable"], "non_canonical"),
        ("anchor_leaf_edited_and_rehashed", "DEC-895 item 2, §11 anchor_root_mismatch: the root no longer covers the leaves", acct, 10, by_anchor,
         (ANCHOR, edited(rows[ANCHOR], leaf_edited, True)), COLD["unreadable"], "anchor_root_mismatch"),
        ("anchor_leaves_reordered_and_rehashed", "DEC-895 item 2, §11 anchor_root_mismatch: the leaves out of stream order", acct, 10, by_anchor,
         (ANCHOR, edited(rows[ANCHOR], leaves_reversed, True)), COLD["unreadable"], "anchor_root_mismatch"),
        ("anchor_unstamped_and_rehashed", "DEC-896, §9.14, DEC-783 item 8: a null token vouches for nothing, the row otherwise sound", acct, 10,
         by_anchor, (ANCHOR, edited(rows[ANCHOR], unstamped, True)), COLD["unreadable"], "no_start"),
    ]
    specs += more_row_specs(rows, by_hash, by_anchor, seg)
    out = []
    for name, clause, stream, n, request, replace, cold, intended in specs:
        case = copy.deepcopy({"name": name, "clause": clause, "stream_id": stream, "from_seq": n, "request": request,
                              "replace": None if replace is None else {"index": replace[0], "row": replace[1]}, "cold": cold})
        expect = start_from_rows(case_rows(rows, case), stream, n, request, cold)
        got = expect["outcome"] if expect["outcome"] != "refused" else expect["cause"]
        if got != intended:
            raise AssertionError(f"{name}: built for {intended}, the reference answers {expect}")
        out.append({**case, "expect": expect})
    return out


def more_row_specs(rows: list[dict], by_hash: dict, by_anchor: dict, seg: dict) -> list[tuple]:
    """E12-3: the resolver's lookup clauses the old resolver's cases alone pinned, each on the rows
    path, a forged row stored consistently (columns from its body, re-hashed) so only the clause
    named can refuse it."""
    acct, agent, tip, unread = ACCOUNT_STREAM, AGENT_STREAM, rows[ANCHOR]["hash"], COLD["unreadable"]
    no_ws, no_ws_too = "acct::A1", "acct:"
    stray = segment(no_ws, 1, 3, GENESIS)
    unhex = segment(acct, 4, 9, "5" * 63)
    leaf_id = {"kind": "anchor", "anchor_event_id": "01J8Z3C6A000000000000000R8"}

    def anchor_with(edit) -> tuple:
        return ANCHOR, edited(rows[ANCHOR], edit, True)

    def manifest(p: dict) -> dict:
        return {"kind": "manifest", "manifest_hash": p["manifest_hash"]}

    return [
        ("segment_of_another_stream_than_requested", "§9.14: a SegmentExported of `s`; this one is the account's, the agent stream asked",
         agent, 4, by_hash, None, cold_read(seg), "no_start"),
        ("segment_requested_before_its_first_seq", "§9.14: a segment starts at its first_seq, not the seq before", acct, 3, by_hash,
         None, cold_read(seg), "no_start"),
        ("segment_requested_after_its_last_seq", "§9.14: a segment starts at its first_seq, not past its last_seq", acct, 10, by_hash,
         None, cold_read(seg), "no_start"),
        ("anchor_requested_at_its_leaf_seq", "§9.14: the leaf's seq is n − 1, not n", acct, 9, by_anchor, None, unread, "no_start"),
        ("anchor_requested_from_seq_zero", "§9.14: no leaf is at seq 0 − 1, though one is stored at seq 0", acct, 0, by_anchor,
         anchor_with(leaf_set(0, seq=0)), unread, "no_start"),
        ("anchor_without_a_leaf_for_the_stream", "§9.14: the leaf for `s`; the agent stream has none, the leaf at n − 1 is the account's",
         agent, 10, by_anchor, None, unread, "no_start"),
        ("anchor_leaf_at_the_seq_is_another_streams", "§9.14: the leaf for `s`; the account's is at 9, the one at 3 the control stream's",
         acct, 4, by_anchor, None, unread, "no_start"),
        ("anchor_moved_to_another_workspace", "§9.14, DEC-767: an anchor of another workspace's control stream, stored consistently",
         acct, 10, by_anchor, anchor_with(moved_to(FOREIGN_CTL)), unread, "no_start"),
        ("anchor_id_names_a_segment", "§9.14: the AnchorComputed with this event_id; it names a SegmentExported", acct, 10,
         {"kind": "anchor", "anchor_event_id": ROW_IDS[SEGMENT]}, None, unread, "no_start"),
        ("anchor_id_names_the_stream_opened", "§9.14: the AnchorComputed with this event_id; it names a StreamOpened", acct, 2,
         {"kind": "anchor", "anchor_event_id": ROW_IDS[OPENED]}, None, unread, "no_start"),
        ("anchor_payload_under_another_event_type", "§9.14: an AnchorComputed; a VerificationRun holding an anchor's payload, chained",
         acct, 10, leaf_id, (FOREIGN, appended(rows[ANCHOR], tip, leaf_id["anchor_event_id"], event_type="VerificationRun")), unread, "no_start"),
        ("segment_for_a_stream_without_a_workspace", "§9.14, DEC-767: `s` has no workspace, so no control stream is its own", no_ws, 1,
         manifest(stray), (FOREIGN, appended(rows[SEGMENT], tip, "01J8Z3C6A000000000000000R9", payload=stray)), cold_read(stray), "no_start"),
        ("anchor_for_a_stream_without_a_workspace", "§9.14, DEC-767: `s` has no workspace, so no control stream is its own", no_ws_too, 10,
         by_anchor, anchor_with(leaf_set(2, stream_id=no_ws_too, seq=9, hash="1" * 64)), unread, "no_start"),
        ("segment_first_prev_hash_not_a_digest", "§9.14, §2: first_prev_hash is 63 hex digits, rule 117 and the row hash hold", acct, 4,
         manifest(unhex), (SEGMENT, edited(rows[SEGMENT], lambda b: b.update(payload=unhex), True)), cold_read(unhex), "no_start"),
        ("anchor_leaf_hash_not_a_digest", "§9.14, §2: the start leaf's hash is not hex, the root and the row hash hold", acct, 10,
         by_anchor, anchor_with(leaf_set(0, hash="g" * 64)), unread, "no_start"),
    ]


def row_answer(starts: dict, c: dict, skip: frozenset[str] = frozenset()) -> dict:
    return start_from_rows(case_rows(starts["rows"], c), c["stream_id"], c["from_seq"], c["request"], c["cold"], skip)


def case_rows(rows: list[dict], case: dict) -> list[dict]:
    out = copy.deepcopy(rows)
    if case["replace"] is not None:
        out[case["replace"]["index"]] = copy.deepcopy(case["replace"]["row"])
    return out


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
    "starts.rows_reference",
    "starts.rows_oracle",
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


def parses_once_canonical(text: str) -> bool:
    """§11 check 1 by `json` alone: no object repeats a key, and the body is its own sorted, compact
    dump."""
    repeats = []

    def pairs(items: list[tuple]) -> dict:
        repeats.extend(k for k, n in collections.Counter(k for k, _ in items).items() if n > 1)
        return dict(items)

    try:
        body = json.loads(text, object_pairs_hook=pairs)
    except ValueError:
        return False
    return not repeats and json.dumps(body, sort_keys=True, separators=(",", ":"), ensure_ascii=False) == text


def anchor_root_holds(p: dict) -> bool:
    """§11's `anchor_root_mismatch` by its own walk: leaves in strictly rising stream byte order, and
    the root rebuilt top down."""
    streams = [leaf["stream_id"].encode() for leaf in p["leaves"]]
    return streams == sorted(set(streams)) and root_by_levels(p["leaves"]) == p["root"]


def is_digest(text) -> bool:
    """A digest as the Rust resolver reads one: a string of exactly 64 lowercase hex digits."""
    return isinstance(text, str) and len(text) == 64 and set(text) <= set("0123456789abcdef")


def row_start_by_oracle(rows: list[dict], case: dict) -> dict:
    """The oracle's own DEC-787 items 3 and 5, written from the decision rather than from the
    reference: find the record as §9.14 does, then judge its row with `json` and `hashlib` directly,
    the manifest hash by `manifest_by_json`, and the cold bytes by their own digest. A row of the
    type the request needs that fails check 1 refuses before any lookup, and an anchor's root is
    rebuilt by `root_by_levels` (DEC-895)."""
    stream, n, request, cold = case["stream_id"], case["from_seq"], case["request"], case["cold"]
    if request["kind"] == "genesis":
        return {"outcome": "start", "from_seq": 1, "prev_hash": GENESIS} if n == 1 else {"outcome": "refused", "cause": "no_start"}
    mine = [r for r in rows if r["stream_id"].split(":")[0] == "ctl" and workspace_of(r["stream_id"]) == workspace_of(stream)]
    needed = "SegmentExported" if request["kind"] == "manifest" else "AnchorComputed"
    if any(r["event_type"] == needed and not parses_once_canonical(r["body"]) for r in mine):
        return {"outcome": "refused", "cause": "non_canonical"}
    hits = []
    for row in mine:
        p = json.loads(row["body"])["payload"]
        fits = (p.get("manifest_hash"), p.get("stream_id"), p.get("first_seq")) == (request.get("manifest_hash"), stream, n)
        if request["kind"] == "manifest" and row["event_type"] == "SegmentExported" and fits and is_digest(p["first_prev_hash"]):
            hits.append((row, p["first_prev_hash"]))
        if request["kind"] == "anchor" and row["event_type"] == "AnchorComputed" and row["event_id"] == request["anchor_event_id"]:
            leaves = [leaf["hash"] for leaf in p["leaves"] if (leaf["stream_id"], leaf["seq"]) == (stream, n - 1) and is_digest(leaf["hash"])]
            hits += [(row, leaves[0])] if p["token"] and leaves else []
    if len(hits) != 1:
        return {"outcome": "refused", "cause": "ambiguous_start" if hits else "no_start"}
    (row, prev), body = hits[0], json.loads(hits[0][0]["body"])
    columns = ("event_id", "event_type", "environment", "prev_hash", "recorded_at", "schema_version", "seq", "stream_id")
    failures = (
        ("non_canonical", json.dumps(body, sort_keys=True, separators=(",", ":"), ensure_ascii=False) != row["body"]),
        ("column_mismatch", any(row[c] != body[c] for c in columns)),
        ("rehash_mismatch", hashlib.sha256(row["body"].encode()).hexdigest() != row["hash"]),
        ("rule_117", row["event_type"] == "SegmentExported" and body["payload"]["manifest_hash"] != manifest_by_json(body["payload"])),
        ("anchor_root_mismatch", row["event_type"] == "AnchorComputed" and not anchor_root_holds(body["payload"])),
    )
    if any(failed for _, failed in failures):
        return {"outcome": "refused", "cause": next(cause for cause, failed in failures if failed)}
    if request["kind"] == "manifest":
        if cold["state"] != "read":
            return {"outcome": "cold_unreadable"}
        if hashlib.sha256(cold["bytes"].encode()).hexdigest() != body["payload"]["manifest_hash"]:
            return {"outcome": "refused", "cause": "cold_manifest_mismatch"}
    return {"outcome": "start", "from_seq": n, "prev_hash": prev}


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
    rows = starts["rows"]
    own = [{"body": json.loads(r["body"]), "hash": r["hash"]} for r in rows if r["stream_id"] == STREAM]
    if chain_breaks(own) or len(own) != len(rows) - 1:
        problems.append(found("starts.rows_oracle", f"the workspace's rows do not chain: {chain_breaks(own)}"))
    for case in starts["row_cases"]:
        got = row_answer(starts, case)
        if got != case["expect"]:
            problems.append(found("starts.rows_reference", f"{case['name']}: expected {case['expect']}, got {got}"))
        if row_start_by_oracle(case_rows(rows, case), case) != case["expect"]:
            problems.append(found("starts.rows_oracle", f"{case['name']}: expected {case['expect']}"))
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
        ("a leaf forged under its row's old hash is expected to start", "starts.rows_reference", mutated(
            lambda s: case(s["trusted_starts"], "row_cases", "anchor_leaf_forged_under_its_old_hash").update(
                expect={"outcome": "start", "from_seq": 10, "prev_hash": FORGED_PREV}))),
        ("the workspace's StreamOpened row is altered after it was hashed", "starts.rows_oracle", mutated(
            lambda s: s["trusted_starts"]["rows"][OPENED].update(body=s["trusted_starts"]["rows"][OPENED]["body"].replace('"control"', '"account"')))),
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
ROW_MUTANTS = ("row.non_canonical", "row.column_mismatch", "row.column_workspace", "row.rehash_mismatch", "row.rule_117",
               "row.anchor_unchecked", "row.first_match", "row.non_ctl_stream", "cold.confirm", "cold.absent_ok", "cold.digest",
               "row.skip_unparsed", "row.anchor_root", "row.anchor_order", "row.unstamped_ok", "row.segment_stream",
               "start.first_seq", "row.anchor_leaf_seq", "row.anchor_seq_floor",
               "row.anchor_leaf_stream", "row.anchor_foreign", "row.anchor_type", "row.bad_stream_id", "row.bad_hex")


def row_mutant_killers(section: dict, mutant: str) -> list[str]:
    """The row cases whose answer the seeded bug `mutant` changes."""
    starts = section["trusted_starts"]
    return [c["name"] for c in starts["row_cases"] if row_answer(starts, c, frozenset([mutant])) != c["expect"]]


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
    for mutant in ROW_MUTANTS:
        if not row_mutant_killers(section, mutant):
            escaped.append(f"cold-records row-start mutant {mutant}")
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
