"""Journal spec v0.38 §11's `segment_rows_mismatch` reference vectors (DEC-894, pending; workspace API
§4.8.1 run order step 2.3a).

A range verified from a trusted start can be rewritten in the hot store and stay consistent from that
start onward: every per-event check passes, and `segment_manifest_mismatch` reads only the cold file.
This range-level check closes that hole. For the trusted-start manifest and every manifest wholly
inside the range, the hot rows `first_seq` to `last_seq` are exported in §6.2's segment form and the
SHA-256 of the bytes is compared with the manifest's `file_sha256`. Rows past `to_seq` come from the
same snapshot and are compared; a segment whose rows the snapshot does not hold in full fails.

This module builds the `segment_rows` section, a section of its own so the Rust tests that read
`cold_records` and `verification_runs` keep their answers until the code change that runs the check
reads this one (ES-22). The hot rows are the account stream's own chain from `account_stream`, cut
into three segments whose cold files are the honest export. It checks the section with oracles of
its own: every snapshot chains from genesis and binds the trusted start, so no per-event check
fails; every manifest matches its cold file, so `segment_manifest_mismatch` passes; and each case's
outcome is recomputed by comparing the cold file's parsed lines with the hot rows one by one rather
than by exporting and hashing. The `VerificationRun` drafts pin rule 111's class for the new code.
Every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
import hashlib
import json

from common import canon, change, rechain, sha256_hex
from control import check_of, draft_for, manifest_hash, reported, violations
from control import invalid as control_invalid
from control import valid as control_valid
from verification import BASES, R0, refs_kept, v1_shape

SPEC = "docs/specs/journal.md v0.38 §11 and workspace API §4.8.1 step 2.3a (DEC-894, pending)"
CHECK = "segment_rows_mismatch"
FAILED = {"check": CHECK, "seq": None}
# The three segments the account stream's eight rows are cut into: (first_seq, last_seq).
SEGMENTS = ((1, 3), (4, 6), (7, 8))
START_SEGMENT = 1


def export(rows: list[dict]) -> bytes:
    """§6.2's segment form: one canonical `{"body", "hash"}` line per row, each ending in LF. The
    line is the canonicalizer's, which `generate.py`'s self-test checks against `export_line_seq_1`."""
    return b"".join((canon({"body": r["body"], "hash": r["hash"]}) + "\n").encode() for r in rows)


def honest_rows(chain: list[dict]) -> list[dict]:
    return [{"body": copy.deepcopy(e["body"]), "hash": e["hash"]} for e in chain]


def segments_of(rows: list[dict]) -> list[dict]:
    """Each segment's manifest, as its `SegmentExported` records it, and the cold file it names."""
    out = []
    for first, last in SEGMENTS:
        held = rows[first - 1 : last]
        data = export(held)
        manifest = {
            "stream_id": held[0]["body"]["stream_id"],
            "first_seq": first,
            "last_seq": last,
            "first_prev_hash": held[0]["body"]["prev_hash"],
            "last_hash": held[-1]["hash"],
            "file_sha256": sha256_hex(data),
        }
        out.append({"manifest": {**manifest, "manifest_hash": manifest_hash(manifest)}, "file": data.decode()})
    return out


def rewritten(rows: list[dict], seq: int) -> list[dict]:
    """The rows with `seq`'s `recorded_at` rewritten and every later row re-chained from it, so the
    stream stays consistent from any trusted start before `seq`."""
    out = copy.deepcopy(rows)
    body = out[seq - 1]["body"]
    body["recorded_at"] = body["recorded_at"][:-4] + "999Z"
    tail = [{"body": r["body"], "hash": r["hash"]} for r in out[seq - 1 :]]
    rechain(tail)
    for row, entry in zip(out[seq - 1 :], tail, strict=True):
        row["hash"] = entry["hash"]
    return out


def start_of(segments: list[dict], kind: str) -> dict:
    if kind == "genesis":
        return {"kind": "genesis", "manifest_hash": None}
    return {"kind": "manifest", "manifest_hash": segments[START_SEGMENT]["manifest"]["manifest_hash"]}


def case(name, clause, honest, rows, start, from_seq, to_seq, fails):
    segments = segments_of(honest)
    return {
        "name": name,
        "clause": clause,
        "rows": rows,
        "segments": segments,
        "start": start_of(segments, start),
        "from_seq": from_seq,
        "to_seq": to_seq,
        "expect": dict(FAILED) if fails else None,
    }


def range_cases(chain: list[dict]) -> list[dict]:
    honest = honest_rows(chain)
    return [
        case("rows_match_their_files", "§11 segment_rows_mismatch: passes", honest, honest, "manifest", 4, 8, False),
        case(
            "start_segment_rewritten",
            "§11 segment_rows_mismatch: a rewrite of the start segment, consistent from the trusted start",
            honest,
            rewritten(honest, 5),
            "manifest",
            4,
            8,
            True,
        ),
        case(
            "in_range_segment_rewritten",
            "§11 segment_rows_mismatch: a rewrite of a segment wholly inside the range",
            honest,
            rewritten(honest, 8),
            "genesis",
            1,
            8,
            True,
        ),
        case(
            "start_segment_rewritten_past_to_seq",
            "workspace API §4.8.1 step 2.3a: the trusted-start segment's rows past to_seq are compared",
            honest,
            rewritten(honest, 6),
            "manifest",
            4,
            5,
            True,
        ),
        case(
            "start_segment_held_in_part",
            "workspace API §4.8.1 step 2.3a: a segment the snapshot does not hold in full fails",
            honest,
            honest[:5],
            "manifest",
            4,
            5,
            True,
        ),
        case(
            "start_segment_past_to_seq_matches",
            "workspace API §4.8.1 step 2.3a: the whole start segment is compared, not the range's part",
            honest,
            honest,
            "manifest",
            4,
            5,
            False,
        ),
        case(
            "segment_past_to_seq_not_judged",
            "workspace API §4.8.1 step 2.3a: a segment that ends past to_seq, not the start's, is not judged",
            honest,
            rewritten(honest, 8),
            "manifest",
            4,
            7,
            False,
        ),
    ]


# --------------------------------------------------------------------------- the reference check

REFERENCE_MUTANTS = ("rows.skip", "rows.start", "rows.inside", "rows.partial", "rows.scope", "rows.clip")


def segment_rows_failure(case: dict, skip: frozenset[str] = frozenset()) -> dict | None:
    """Step 2.3a: the trusted-start manifest and every manifest wholly inside the range, in
    ascending `first_seq`; each segment's hot rows exported and hashed against `file_sha256`."""
    if "rows.skip" in skip:
        return None
    lo, hi = case["from_seq"], case["to_seq"]
    start = case["start"]["manifest_hash"]
    judged = []
    for segment in case["segments"]:
        m = segment["manifest"]
        is_start = m["manifest_hash"] == start and "rows.start" not in skip
        inside = lo <= m["first_seq"] and m["last_seq"] <= hi and "rows.inside" not in skip
        if is_start or inside or "rows.scope" in skip:
            judged.append(m)
    by_seq = {r["body"]["seq"]: r for r in case["rows"]}
    for m in sorted(judged, key=lambda m: m["first_seq"]):
        first, last = m["first_seq"], m["last_seq"]
        if "rows.clip" in skip:
            first, last = max(first, lo), min(last, hi)
        wanted = range(first, last + 1)
        if any(s not in by_seq for s in wanted):
            if "rows.partial" in skip:
                continue
            return dict(FAILED)
        if sha256_hex(export([by_seq[s] for s in wanted])) != m["file_sha256"]:
            return dict(FAILED)
    return None


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = (
    "rows.chain",
    "rows.manifest",
    "rows.reference",
    "rows.lines",
    "drafts.valid",
    "invalid_drafts",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def text_of(body: dict) -> str:
    return json.dumps(body, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def chain_problems(case: dict) -> list[str]:
    """The snapshot passes checks 3 to 5 from genesis, so its prefix verifies and every per-event
    check of the range passes, and the trusted start's `prev_hash` is the hash of `from_seq − 1`."""
    out, prev = [], "0" * 64
    for i, row in enumerate(case["rows"], start=1):
        body = row["body"]
        if body["seq"] != i or body["prev_hash"] != prev:
            out.append(f"seq {i} does not link")
        if hashlib.sha256(text_of(body).encode()).hexdigest() != row["hash"]:
            out.append(f"seq {i} does not re-hash")
        prev = row["hash"]
    hashes = {r["body"]["seq"]: r["hash"] for r in case["rows"]}
    start = case["start"]
    if start["kind"] == "manifest":
        manifest = next(s["manifest"] for s in case["segments"] if s["manifest"]["manifest_hash"] == start["manifest_hash"])
        bound = manifest["first_seq"] == case["from_seq"] and hashes.get(case["from_seq"] - 1) == manifest["first_prev_hash"]
    else:
        bound = case["from_seq"] == 1
    if not bound:
        out.append("the trusted start is not bound to the snapshot")
    if max(hashes, default=0) < case["to_seq"]:
        out.append("the snapshot ends before to_seq")
    return out


def manifest_problems(segment: dict) -> list[str]:
    """`segment_manifest_mismatch` passes: the file hashes to `file_sha256`, its own lines give the
    manifest's edges, and the manifest hash is its six fields' (rule 117), from `json.dumps`."""
    m, data = segment["manifest"], segment["file"].encode()
    lines = [json.loads(line) for line in segment["file"].split("\n")[:-1]]
    six = {
        "file_sha256": m["file_sha256"],
        "first_prev_hash": m["first_prev_hash"],
        "first_seq": m["first_seq"],
        "last_hash": m["last_hash"],
        "last_seq": m["last_seq"],
        "stream": m["stream_id"],
    }
    edges = (
        lines[0]["body"]["seq"],
        lines[-1]["body"]["seq"],
        lines[0]["body"]["prev_hash"],
        lines[-1]["hash"],
        lines[0]["body"]["stream_id"],
    )
    out = []
    if hashlib.sha256(data).hexdigest() != m["file_sha256"] or not segment["file"].endswith("\n"):
        out.append(f"segment {m['first_seq']}: the file is not the one its manifest names")
    if edges != (m["first_seq"], m["last_seq"], m["first_prev_hash"], m["last_hash"], m["stream_id"]):
        out.append(f"segment {m['first_seq']}: the file's lines do not give the manifest's edges")
    if hashlib.sha256(json.dumps(six, separators=(",", ":")).encode()).hexdigest() != m["manifest_hash"]:
        out.append(f"segment {m['first_seq']}: manifest hash")
    return out


def by_lines(case: dict) -> dict | None:
    """The oracle's own step 2.3a, written from the paragraph: the segments the range starts from or
    holds, each cold line parsed and compared, by value, with the hot row of its `seq`, and every
    `seq` of the segment held."""
    start, lo, hi = case["start"], case["from_seq"], case["to_seq"]
    rows = {r["body"]["seq"]: r for r in case["rows"]}
    for segment in case["segments"]:
        m = segment["manifest"]
        if not (m["manifest_hash"] == start["manifest_hash"] or (m["first_seq"] >= lo and m["last_seq"] <= hi)):
            continue
        if any(s not in rows for s in range(m["first_seq"], m["last_seq"] + 1)):
            return dict(FAILED)
        for line in segment["file"].split("\n")[:-1]:
            cold = json.loads(line)
            hot = rows[cold["body"]["seq"]]
            if hot["hash"] != cold["hash"] or text_of(hot["body"]) != text_of(cold["body"]):
                return dict(FAILED)
    return None


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for c in section["ranges"]:
        problems += [found("rows.chain", f"{c['name']}: {p}") for p in chain_problems(c)]
        for segment in c["segments"]:
            problems += [found("rows.manifest", f"{c['name']}: {p}") for p in manifest_problems(segment)]
        if segment_rows_failure(c) != c["expect"]:
            problems.append(found("rows.reference", f"{c['name']}: expected {c['expect']}"))
        if by_lines(c) != c["expect"]:
            problems.append(found("rows.lines", f"{c['name']}: expected {c['expect']}"))
    for c in section["valid_drafts"]:
        got = violations(draft_for(section, c))
        if got:
            problems.append(found("drafts.valid", f"{c['name']}: expected Valid, got {got}"))
    for c in section["invalid_drafts"]:
        got = violations(draft_for(section, c))
        if not reported(got, c["expect"]):
            problems.append(found("invalid_drafts", f"{c['name']}: expected {c['expect']}, got {got}"))
    return problems


# --------------------------------------------------------------------------- VerificationRun drafts

FAILED_RANGE = (change(f"{R0}.failure", dict(FAILED)), change(f"{R0}.to_hash", "1" * 64), change(f"{R0}.checked", 40))


def drafts() -> tuple[list[dict], list[dict]]:
    """Rule 111 for the new code at version 2, and version 1's closed list without it."""
    bad = "run_failed"
    v1 = copy.deepcopy(BASES[bad]["payload"]["ranges"])
    v1[0].update(failure=dict(FAILED), to_hash="1" * 64)
    valid = [
        control_valid(
            "rows_mismatch_for_the_range",
            "§9.13 rule 111: `segment_rows_mismatch` is reported for the range, with seq null (DEC-894)",
            bad,
            refs_kept(bad, list(FAILED_RANGE)),
        ),
    ]
    invalid = [
        control_invalid(
            "rows_mismatch_at_a_seq",
            "§9.13 rule 111: `segment_rows_mismatch` names no seq",
            bad,
            refs_kept(bad, [*FAILED_RANGE, change(f"{R0}.failure", {"check": CHECK, "seq": 17})]),
            "schema",
            f"{R0}.failure.seq",
        ),
        control_invalid(
            "rows_mismatch_at_version_1",
            "§9.13: version 1's closed list of checks does not gain `segment_rows_mismatch` (§8)",
            bad,
            refs_kept(bad, [change("schema_version", 1), change("payload.ranges", v1_shape(v1))]),
            "non_canonical",
            f"{R0}.failure.check",
        ),
    ]
    return valid, invalid


# --------------------------------------------------------------------------- seeded bugs


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def named(s, name):
        return next(c for c in s["ranges"] if c["name"] == name)

    def unchained(s):
        named(s, "rows_match_their_files")["rows"][4]["body"]["recorded_at"] = "2026-09-21T14:00:20.000000999Z"

    def file_edited(s):
        segment = named(s, "segment_past_to_seq_not_judged")["segments"][0]
        segment["file"] = segment["file"].replace("StreamOpened", "StreamClosed")

    def seq_named(s):
        ranges = next(c for c in s["valid_drafts"][0]["changes"] if c["path"] == "payload.ranges")
        ranges["value"][0]["failure"]["seq"] = 9

    return [
        ("a snapshot row is altered after it was hashed", "rows.chain", mutated(unchained)),
        ("a cold file differs from its manifest", "rows.manifest", mutated(file_edited)),
        (
            "a rewritten start segment is expected to pass",
            "rows.reference",
            mutated(lambda s: named(s, "start_segment_rewritten").update(expect=None)),
        ),
        (
            "a clean range is expected to fail",
            "rows.lines",
            mutated(lambda s: named(s, "rows_match_their_files").update(expect=dict(FAILED))),
        ),
        (
            "the valid draft names a seq",
            "drafts.valid",
            mutated(seq_named),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: s["invalid_drafts"][0]["expect"].update(path=f"{R0}.failure.check")),
        ),
    ]


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught; a vector mutant only by the check it is registered against."""
    escaped = []
    for mutant in REFERENCE_MUTANTS:
        skip = frozenset([mutant])
        if all(segment_rows_failure(c, skip) == c["expect"] for c in section["ranges"]):
            escaped.append(f"segment_rows reference mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"segment_rows check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"segment_rows vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section(chain: list[dict]) -> dict:
    valid, invalid = drafts()
    return {
        "spec": SPEC,
        "ranges": range_cases(chain),
        "drafts": {"run_failed": copy.deepcopy(BASES["run_failed"])},
        "valid_drafts": valid,
        "invalid_drafts": invalid,
    }
