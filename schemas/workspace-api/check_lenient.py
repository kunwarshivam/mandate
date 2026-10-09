"""Checks DEC-682 item 27's API-7 leniency.

Run from the repository root, in the reference environment:

    python3 -I schemas/workspace-api/check_lenient.py

1. The schemas carrying `"x-api7-lenient": true` are exactly the six below, and each one's operation
   is named in workspace API-7's row. API-7's other operations are listed with why they have no
   lenient schema.
2. Every case of `examples/lenient/api7.json` is for one of the six:
   - an `accept` case's pointers each name a member present in its body; a body that is not JSON
     drops `""`, and only for pause and hold; a non-empty list is a valid `dropped` of the
     operation's `202` schema;
   - a `refuse` case's body fails the operation's strict schema, or is not JSON.
"""

import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / "schemas/workspace-api"

# operation: (schema, the words API-7's row names it by)
LENIENT = {
    "pause": ("commands/pause-request", "Pause"),
    "hold": ("commands/hold-request", "holding new openings"),
    "kill_switch": ("commands/kill-switch-request", "the kill switch at any scope"),
    "owner_exit": ("commands/owner-exit-request", "an owner exit"),
    "respond_approval": ("commands/approval-response-request", "Skip on an approval"),
    "end_delegation": ("commands/end-delegation-request", "ending a delegation"),
}
# API-7's other operations, with why none has a lenient schema.
NOT_LENIENT = {
    "away mode": "it has no request schema yet",
    "the kill-switch half of a revoke on compromise": "it is the revoke's own strict body",
}
UNPARSABLE_OK = {"pause", "hold"}
# The `202` schema whose `dropped` lists an operation's accept case; the rest use command-accepted.
ACCEPTED = {"respond_approval": "commands/approval-response-accepted"}


def resolves(body, pointer: str) -> bool:
    if pointer == "":
        return False
    node = body
    for part in pointer.lstrip("/").split("/"):
        part = part.replace("~1", "/").replace("~0", "~")
        if isinstance(node, dict) and part in node:
            node = node[part]
        elif isinstance(node, list) and part.isdigit() and int(part) < len(node):
            node = node[int(part)]
        else:
            return False
    return True


def main() -> int:
    problems: list[str] = []
    schemas = {
        p.relative_to(HERE).as_posix().removesuffix(".schema.json"): json.loads(
            p.read_text()
        )
        for p in HERE.rglob("*.schema.json")
    }
    mandate = json.loads((ROOT / "schemas/mandate.schema.json").read_text())
    registry = Registry().with_resources(
        (doc["$id"], Resource.from_contents(doc))
        for doc in [*schemas.values(), mandate]
    )
    flagged = {
        name for name, doc in schemas.items() if doc.get("x-api7-lenient") is True
    }
    expected = {schema for schema, _ in LENIENT.values()}
    if flagged != expected:
        problems.append(
            f"x-api7-lenient on {sorted(flagged)}, expected {sorted(expected)}"
        )
    spec = (ROOT / "docs/specs/workspace-api.md").read_text(encoding="utf-8")
    row = next(
        (line for line in spec.splitlines() if line.startswith("| **API-7** |")), ""
    )
    for operation, (_, words) in LENIENT.items():
        if words not in row:
            problems.append(f"API-7's row does not name {operation} ({words!r})")
    for words in NOT_LENIENT:
        if words not in row:
            problems.append(
                f"API-7's row does not name {words!r}, listed as not lenient"
            )
    cases = json.loads((HERE / "examples/lenient/api7.json").read_text())["cases"]
    checked = 0
    for i, case in enumerate(cases):
        checked += 1
        label = f"case {i} ({case['operation']})"
        if case["operation"] not in LENIENT:
            problems.append(f"{label}: not one of the six lenient operations")
            continue
        schema = schemas[LENIENT[case["operation"]][0]]
        body = case["body"]
        raw = isinstance(body, str)
        if "accept" in case:
            accepted = schemas[ACCEPTED.get(case["operation"], "commands/command-accepted")]
            if case["accept"] and not Draft202012Validator(
                accepted["properties"]["dropped"], registry=registry
            ).is_valid(case["accept"]):
                problems.append(
                    f"{label}: {case['accept']!r} is not a valid `dropped` of its 202"
                )
            if raw:
                if case["operation"] not in UNPARSABLE_OK or case["accept"] != [""]:
                    problems.append(
                        f'{label}: only pause and hold read a non-JSON body, as {{}} with [""]'
                    )
            else:
                for pointer in case["accept"]:
                    if not resolves(body, pointer):
                        problems.append(
                            f"{label}: dropped {pointer!r} is not in the body"
                        )
        elif "refuse" in case:
            if not raw and Draft202012Validator(schema, registry=registry).is_valid(
                body
            ):
                problems.append(f"{label}: refused, yet its strict schema accepts it")
        else:
            problems.append(f"{label}: neither accept nor refuse")
    for problem in problems:
        print(f"FAIL {problem}")
    print(
        f"{checked} cases and {len(LENIENT)} lenient schemas checked; {len(problems)} problems"
    )
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
