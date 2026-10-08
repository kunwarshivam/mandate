"""Mutation sweep over the envelope and command schemas (DEC-682 item 31).

Run from the repository root, in the reference environment:

    python3 -I schemas/workspace-api/mutate_schemas.py [--all]

Each mutant changes one constraint of one schema file: it drops a required member, a value of an
enum, a `const`, `additionalProperties` or `unevaluatedProperties`, a pattern, a bound, a branch of
a `oneOf`, a `then` or `else`, an `allOf` entry, or an `x-api7-lenient` flag. The checkers
(`check_examples.py`, `check_planned.py`, `check_lenient.py`) then run over a copy of the tree. A mutant it still passes is a survivor: an example is missing. The
sweep fails on any survivor not listed in `DELIBERATE` with its reason; `--all` prints them all.
"""

import copy
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / "schemas/workspace-api"
FILES = [
    "envelope.schema.json",
    *sorted(f"commands/{p.name}" for p in (HERE / "commands").glob("*.schema.json")),
]

CHECKERS = ["check_examples.py", "check_planned.py", "check_lenient.py"]

# Survivors that no example can kill, each with its reason.
DELIBERATE: dict[str, str] = {
    "envelope.schema.json/$defs/Problem/required drops code": "without `code`, every per-code pin applies at once, which no instance satisfies",
    "envelope.schema.json/$defs/Problem/properties/type/minLength removed": "every code pins `type` to a non-empty constant",
    "envelope.schema.json/$defs/Problem/properties/title/minLength removed": "every code pins `title` to a non-empty constant",
    "envelope.schema.json/$defs/Problem/properties/status/enum gains a value": "every code pins `status` to one constant",
    "envelope.schema.json/$defs/Problem/allOf/8/then/properties/effect/enum gains a value": "`effect` is also `Effect`, whose own enum refuses any added value",
    "envelope.schema.json/$defs/Problem/allOf/10/then/properties/effect/enum gains a value": "`effect` is also `Effect`, whose own enum refuses any added value",
    "envelope.schema.json/$defs/Problem/allOf/21/then/properties/effect/enum gains a value": "`effect` is also `Effect`, whose own enum refuses any added value",
    "envelope.schema.json/$defs/Problem/allOf/22/then/properties/effect/enum gains a value": "`effect` is also `Effect`, whose own enum refuses any added value",
    "envelope.schema.json/$defs/Problem/allOf/23/then/properties/effect/enum gains a value": "`effect` is also `Effect`, whose own enum refuses any added value",
    "envelope.schema.json/$defs/Problem/allOf/24/then/properties/effect/enum gains a value": "`effect` is also `Effect`, whose own enum refuses any added value",
    "envelope.schema.json/$defs/Problem/allOf/25/then/properties/effect/enum gains a value": "`effect` is also `Effect`, whose own enum refuses any added value",
}


def mutants(node, path=""):
    """(label, mutate) pairs, where mutate(doc) changes the node at `path` in a copy of `doc`."""
    if isinstance(node, dict):
        for key, value in node.items():
            here = f"{path}/{key}"
            if key == "required" and isinstance(value, list):
                for member in value:
                    yield (
                        f"{here} drops {member}",
                        lambda n, m=member: n["required"].remove(m),
                    )
            elif key == "enum" and isinstance(value, list):
                for item in value:
                    yield (
                        f"{here} drops {item!r}",
                        lambda n, i=item: n["enum"].remove(i),
                    )
                yield (
                    f"{here} gains a value",
                    lambda n: n["enum"].append("mutant_value"),
                )
            elif key == "const":
                yield f"{here} changes", lambda n: n.update(const=_other(n["const"]))
            elif (
                key in ("additionalProperties", "unevaluatedProperties")
                and value is False
            ):
                yield f"{here} removed", lambda n, k=key: n.pop(k)
            elif key == "pattern":
                yield f"{here} becomes .*", lambda n: n.update(pattern=".*")
            elif key in (
                "minimum",
                "maximum",
                "minItems",
                "maxItems",
                "uniqueItems",
                "minLength",
            ) or key in ("then", "else"):
                yield f"{here} removed", lambda n, k=key: n.pop(k)
            elif key == "oneOf" and isinstance(value, list) and len(value) > 1:
                for i in range(len(value)):
                    yield f"{here}/{i} removed", lambda n, i=i: n["oneOf"].pop(i)
            elif key == "allOf" and isinstance(value, list):
                for i in range(len(value)):
                    yield f"{here}/{i} removed", lambda n, i=i: n["allOf"].pop(i)
            for label, mutate in mutants(value, here):
                yield label, _descend(key, mutate)
    elif isinstance(node, list):
        for i, value in enumerate(node):
            for label, mutate in mutants(value, f"{path}/{i}"):
                yield label, _descend(i, mutate)


def _descend(key, mutate):
    return lambda n: mutate(n[key])


def _other(value):
    if isinstance(value, bool):
        return not value
    if isinstance(value, int):
        return value + 1
    return f"{value}_mutant"


def survivors(show_all: bool) -> list[str]:
    found = []
    with tempfile.TemporaryDirectory() as tmp:
        tree = Path(tmp) / "schemas"
        shutil.copytree(ROOT / "schemas", tree)
        shutil.copytree(ROOT / "docs/specs", Path(tmp) / "docs/specs")
        (Path(tmp) / "docs/project").mkdir(parents=True)
        shutil.copy(ROOT / "docs/project/06-backlog-v1.md", Path(tmp) / "docs/project")
        checkers = [tree / f"workspace-api/{c}" for c in CHECKERS]
        for name in FILES:
            target = tree / "workspace-api" / name
            original = json.loads((HERE / name).read_text(encoding="utf-8"))
            for label, mutate in list(mutants(original)):
                doc = copy.deepcopy(original)
                mutate(doc)
                target.write_text(json.dumps(doc), encoding="utf-8")
                caught = any(
                    subprocess.run(
                        [sys.executable, "-I", str(checker)],
                        capture_output=True,
                        check=False,
                    ).returncode
                    for checker in checkers
                )
                if not caught:
                    found.append(f"{name}{label}")
            target.write_text(json.dumps(original), encoding="utf-8")
    if show_all:
        return found
    return [s for s in found if s not in DELIBERATE]


def main() -> int:
    show_all = "--all" in sys.argv
    found = survivors(show_all)
    for survivor in found:
        print(f"SURVIVED {survivor}")
    print(f"{len(found)} survivors")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
