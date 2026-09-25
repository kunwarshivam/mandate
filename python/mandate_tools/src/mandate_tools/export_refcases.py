"""Exports the reference-case YAML files to JSON fixtures for the Rust harness (ADR-0001 ES-11).

PyYAML is the loader the reference implementation uses, so its reading of the YAML is the approved
meaning. The loader here is stricter: anything a different YAML loader could read differently is an
error instead of a silent reinterpretation.
"""

import argparse
import json
import re
import sys
from pathlib import Path

import yaml

SOURCES = ("trading-domain", "journal", "mandate")
PLAIN_INT = re.compile(r"-?(0|[1-9][0-9]*)")


class ExportError(ValueError):
    pass


class StrictLoader(yaml.SafeLoader):
    """SafeLoader that rejects duplicate keys and every scalar type YAML loaders disagree on."""


def _mapping(loader: StrictLoader, node: yaml.MappingNode, deep: bool = False) -> dict:
    loader.flatten_mapping(node)
    seen = set()
    for key_node, _ in node.value:
        key = loader.construct_object(key_node, deep=deep)
        if key in seen:
            raise ExportError(f"duplicate key {key!r} at {key_node.start_mark}")
        seen.add(key)
    return yaml.SafeLoader.construct_mapping(loader, node, deep=deep)


def _reject(kind: str):
    def construct(loader: StrictLoader, node: yaml.ScalarNode):
        raise ExportError(f"{kind} scalar {node.value!r} at {node.start_mark}; quote it as a string")

    return construct


def _bool(loader: StrictLoader, node: yaml.ScalarNode) -> bool:
    if node.value not in ("true", "false"):
        raise ExportError(f"boolean spelled {node.value!r} at {node.start_mark}; use true or false")
    return node.value == "true"


def _int(loader: StrictLoader, node: yaml.ScalarNode) -> int:
    if not PLAIN_INT.fullmatch(node.value):
        raise ExportError(f"non-decimal integer {node.value!r} at {node.start_mark}; use plain decimal")
    return int(node.value)


StrictLoader.add_constructor(yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, _mapping)
StrictLoader.add_constructor("tag:yaml.org,2002:float", _reject("float"))
StrictLoader.add_constructor("tag:yaml.org,2002:timestamp", _reject("timestamp"))
StrictLoader.add_constructor("tag:yaml.org,2002:bool", _bool)
StrictLoader.add_constructor("tag:yaml.org,2002:int", _int)


def load(text: str):
    return yaml.load(text, Loader=StrictLoader)


def render(data) -> str:
    return json.dumps(data, indent=1, ensure_ascii=False) + "\n"


def export(root: Path, out: Path) -> list[Path]:
    out.mkdir(parents=True, exist_ok=True)
    written = []
    for name in SOURCES:
        source = root / "docs" / "specs" / "reference-cases" / f"{name}.yaml"
        target = out / f"{name}.json"
        target.write_text(render(load(source.read_text(encoding="utf-8"))), encoding="utf-8")
        written.append(target)
    return written


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, required=True, help="directory to write the JSON fixtures to")
    parser.add_argument(
        "--root", type=Path, default=Path(__file__).resolve().parents[4], help="repository root"
    )
    args = parser.parse_args(argv)
    try:
        for path in export(args.root, args.out):
            print(path)
    except ExportError as err:
        print(f"export_refcases: {err}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
