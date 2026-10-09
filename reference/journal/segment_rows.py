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
into three segments whose cold files are the honest export. The `VerificationRun` drafts pin rule
111's class for the new code. Every seeded bug in the reference check is shown caught.
"""

from __future__ import annotations

import copy

from common import canon, change, rechain, sha256_hex
from control import draft_for, manifest_hash, reported, violations
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


# --------------------------------------------------------------------------- the section's checks


def check_section(section: dict) -> list[str]:
    """Every failure: each range's outcome under the reference check, and each draft's."""
    problems = []
    for c in section["ranges"]:
        if segment_rows_failure(c) != c["expect"]:
            problems.append(f"segment_rows {c['name']}: expected {c['expect']}")
    for c in section["valid_drafts"]:
        got = violations(draft_for(section, c))
        if got:
            problems.append(f"segment_rows {c['name']}: expected Valid, got {got}")
    for c in section["invalid_drafts"]:
        got = violations(draft_for(section, c))
        if not reported(got, c["expect"]):
            problems.append(f"segment_rows {c['name']}: expected {c['expect']}, got {got}")
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


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug in the reference check caught by some range."""
    escaped = []
    for mutant in REFERENCE_MUTANTS:
        skip = frozenset([mutant])
        if all(segment_rows_failure(c, skip) == c["expect"] for c in section["ranges"]):
            escaped.append(f"segment_rows reference mutant {mutant}")
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
