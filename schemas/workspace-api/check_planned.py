"""Checks DEC-683's planned markers.

Run from the repository root, in the reference environment:

    python3 -I schemas/workspace-api/check_planned.py

1. Every `(planned: <story>)` in `docs/specs/*.md`, and every story in a schema's `x-planned`
   object (`{"<value>": "<story>"}`), names a story the backlog lists as `- **<story> ...`.
2. For each spec table paired with a schema enum below, the table's codes equal the enum's values,
   and the rows the table marks planned equal the enum's `x-planned`, story for story.
3. Every `x-planned` value is one of its enum's values.
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / "schemas/workspace-api"
BACKLOG = ROOT / "docs/project/06-backlog-v1.md"
MARKER = re.compile(r"\(planned: ([^)]+)\)")

# (spec file, section start, section end, schema file, JSON pointer to the enum's object)
PAIRS = [
    (
        "docs/specs/workspace-api.md",
        "### 3.5 Errors",
        "### 3.6",
        "envelope.schema.json",
        "/$defs/ProblemCode",
    ),
]


def pointer(doc, path: str):
    for part in path.strip("/").split("/"):
        doc = doc[part.replace("~1", "/").replace("~0", "~")]
    return doc


def table_codes(text: str) -> dict[str, str | None]:
    """Each code of a table whose second cell is an HTTP status, with its row's planned story."""
    codes: dict[str, str | None] = {}
    for line in text.splitlines():
        cells = [c.strip() for c in line.split("|")]
        if len(cells) < 4 or not cells[2].isdigit():
            continue
        marker = MARKER.search(line)
        for code in re.findall(r"`([^`]+)`", cells[1]):
            codes[code] = marker.group(1) if marker else None
    return codes


def planned_objects(node, path=""):
    """Every object carrying `x-planned`, with its pointer."""
    if isinstance(node, dict):
        if "x-planned" in node:
            yield path, node
        for key, value in node.items():
            yield from planned_objects(value, f"{path}/{key}")
    elif isinstance(node, list):
        for i, value in enumerate(node):
            yield from planned_objects(value, f"{path}/{i}")


def main() -> int:
    problems: list[str] = []
    stories = set(
        re.findall(
            r"^- \*\*(E\d+-\d+)\b", BACKLOG.read_text(encoding="utf-8"), re.MULTILINE
        )
    )
    checked = 0
    for spec in sorted((ROOT / "docs/specs").glob("*.md")):
        for story in MARKER.findall(spec.read_text(encoding="utf-8")):
            checked += 1
            if story not in stories:
                problems.append(
                    f"{spec.relative_to(ROOT)}: (planned: {story}) names no backlog story"
                )
    for path in sorted(HERE.rglob("*.schema.json")):
        doc = json.loads(path.read_text(encoding="utf-8"))
        for where, node in planned_objects(doc):
            planned = node["x-planned"]
            if not isinstance(planned, dict):
                problems.append(
                    f"{path.name}{where}: x-planned must map each value to its story"
                )
                continue
            for value, story in planned.items():
                checked += 1
                if story not in stories:
                    problems.append(
                        f"{path.name}{where}: x-planned {value} names no backlog story {story}"
                    )
                if value not in node.get("enum", []):
                    problems.append(
                        f"{path.name}{where}: x-planned {value} is not in the enum"
                    )
    for spec, start, end, schema, at in PAIRS:
        text = (ROOT / spec).read_text(encoding="utf-8")
        section = text.split(start, 1)[1].split(end, 1)[0]
        table = table_codes(section)
        node = pointer(json.loads((HERE / schema).read_text(encoding="utf-8")), at)
        checked += 1
        if set(table) != set(node["enum"]):
            problems.append(
                f"{spec} {start} and {schema}{at} list different codes: "
                f"only in the spec {sorted(set(table) - set(node['enum']))}, "
                f"only in the schema {sorted(set(node['enum']) - set(table))}"
            )
        spec_planned = {code: story for code, story in table.items() if story}
        if spec_planned != node.get("x-planned", {}):
            problems.append(
                f"{spec} {start} marks {spec_planned} planned, {schema}{at} marks {node.get('x-planned', {})}"
            )
    for problem in problems:
        print(f"FAIL {problem}")
    print(f"{checked} markers and pairs checked; {len(problems)} problems")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
