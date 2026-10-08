"""Checks the workspace API schemas against their examples (DEC-740).

Run from the repository root, in the reference environment (jsonschema 4.26.0, pinned in
`reference/mandate/requirements.txt`):

    python3 -I schemas/workspace-api/check_examples.py

1. Every `*.schema.json` under `schemas/workspace-api/` is a valid JSON Schema 2020-12 document,
   and its `$id` is `https://mandate.dev/schemas/workspace-api/v1/` plus its path from
   `schemas/workspace-api/`. A relative `$ref` such as `../common.schema.json#/$defs/Decimal`
   from `read-models/` then resolves the way the file tree reads. `$ref`s resolve against those
   files and `schemas/mandate.schema.json`, by `$id`.
2. Every `examples/<name>.json` must be valid. `<name>` is a schema file's path relative to
   `schemas/workspace-api/`, without `.schema.json`; a `/` in that path is `.` in the example's name,
   so `read-models/agent.schema.json` has `examples/read-models.agent.json`.
   - For a schema of `$defs` only, the example is `{"<Def>": [instance, ...]}`. Every definition
     needs at least one instance.
   - Otherwise the example is one instance of the whole schema.
3. Every `examples/<name>.invalid.json` must be rejected, case by case.
   - For a `$defs`-only schema: `{"<Def>": [instance, ...]}`.
   - Otherwise: `[{"label": ..., "set": {"<JSON pointer>": value}, "remove": ["<pointer>"]}]`.
     Each case is applied to a copy of the valid example.
   A case the schema accepts is a failure, so each one shows the schema refuses what it names.
"""

import copy
import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / "schemas/workspace-api"
EXAMPLES = HERE / "examples"
ID_BASE = "https://mandate.dev/schemas/workspace-api/v1/"


def load(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def pointer_parts(pointer: str) -> list[str]:
    if not pointer.startswith("/"):
        raise ValueError(f"not a JSON pointer: {pointer!r}")
    return [p.replace("~1", "/").replace("~0", "~") for p in pointer[1:].split("/")]


def walk(doc, parts: list[str]):
    for part in parts:
        doc = doc[int(part)] if isinstance(doc, list) else doc[part]
    return doc


def apply_case(body, case: dict):
    body = copy.deepcopy(body)
    for pointer, value in case.get("set", {}).items():
        *parent, last = pointer_parts(pointer)
        target = walk(body, parent)
        if isinstance(target, list):
            target[int(last)] = value
        else:
            target[last] = value
    for pointer in case.get("remove", []):
        *parent, last = pointer_parts(pointer)
        target = walk(body, parent)
        del target[int(last) if isinstance(target, list) else last]
    return body


def defs_only(schema: dict) -> bool:
    return set(schema) <= {"$schema", "$id", "title", "description", "$defs"}


def main() -> int:
    paths = sorted(HERE.rglob("*.schema.json"))
    schemas = {p.relative_to(HERE).as_posix().removesuffix(".schema.json"): load(p) for p in paths}
    mandate = load(ROOT / "schemas/mandate.schema.json")
    registry = Registry().with_resources(
        (doc["$id"], Resource.from_contents(doc)) for doc in [*schemas.values(), mandate]
    )
    problems: list[str] = []
    checked = 0

    def validator(schema: dict, definition: str | None = None) -> Draft202012Validator:
        root = {"$ref": f"{schema['$id']}#/$defs/{definition}"} if definition else schema
        return Draft202012Validator(root, registry=registry)

    def expect(valid: bool, v: Draft202012Validator, instance, label: str) -> None:
        nonlocal checked
        checked += 1
        errors = list(v.iter_errors(instance))
        if valid and errors:
            problems.append(f"{label}: should be valid: {errors[0].message[:160]}")
        if not valid and not errors:
            problems.append(f"{label}: should be rejected, was accepted")

    for name, schema in sorted(schemas.items()):
        Draft202012Validator.check_schema(schema)
        if schema.get("$id") != f"{ID_BASE}{name}.schema.json":
            problems.append(f"{name}: $id must be {ID_BASE}{name}.schema.json")
            continue
        stem = name.replace("/", ".")
        good_path, bad_path = EXAMPLES / f"{stem}.json", EXAMPLES / f"{stem}.invalid.json"
        if not good_path.exists():
            problems.append(f"{name}: no example at {good_path.relative_to(ROOT)}")
            continue
        good = load(good_path)
        bad = load(bad_path) if bad_path.exists() else None
        if defs_only(schema):
            for definition in schema["$defs"]:
                if not good.get(definition):
                    problems.append(f"{name}: no valid instance of {definition}")
            for definition, instances in good.items():
                for i, instance in enumerate(instances):
                    expect(True, validator(schema, definition), instance, f"{name}#{definition}[{i}]")
            for definition, instances in (bad or {}).items():
                for i, instance in enumerate(instances):
                    label = f"{name}#{definition} invalid[{i}]"
                    expect(False, validator(schema, definition), instance, label)
        else:
            expect(True, validator(schema), good, name)
            for case in bad or []:
                expect(False, validator(schema), apply_case(good, case), f"{name}: {case['label']}")

    for path in sorted(EXAMPLES.glob("*.json")):
        stem = path.name.removesuffix(".json").removesuffix(".invalid")
        if stem.replace(".", "/") not in schemas:
            problems.append(f"{path.relative_to(ROOT)}: no schema for this example")

    for problem in problems:
        print(f"FAIL {problem}")
    print(f"{checked} instances checked across {len(schemas)} schemas; {len(problems)} problems")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
