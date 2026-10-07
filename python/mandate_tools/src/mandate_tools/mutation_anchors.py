"""Checks that every mutation anchor in reference/mandate/mutants.py still occurs in ref.py.

`mutants.py` seeds each bug by replacing an `old` text in a copy of `ref.py` with a `new` one, and
asserts before its first run that every `old` text is there. Only the nightly runs it, since the
sweep takes most of an hour, so an anchor that an edit to `ref.py` left behind passed every per-PR
job (#443 round 3). This is that first assertion on its own: `cargo xtask ci reference` runs it per
PR in well under a second, without the sweep. It reads every mutant table the module defines, a
dict of `(old, new)` string pairs, so a table added later is covered without a change here, and it
refuses a module in which it finds no table, so a reshaped `mutants.py` fails loudly rather than
passing an empty check.

Usage: `python -m mandate_tools.mutation_anchors [DIR]`, where DIR holds `mutants.py` and `ref.py`
(the default is the repository's `reference/mandate/`).
"""

import argparse
import importlib.util
import sys
from pathlib import Path
from types import ModuleType

ROOT = Path(__file__).resolve().parents[4]
REFERENCE = ROOT / "reference/mandate"
MUTANTS = "mutants.py"
REFERENCE_MODEL = "ref.py"


class AnchorError(ValueError):
    pass


def load_mutants(directory: Path) -> ModuleType:
    """Imports DIR's `mutants.py` without running it; its tables are module constants."""
    path = directory / MUTANTS
    spec = importlib.util.spec_from_file_location("mutants", path)
    if spec is None or spec.loader is None:
        raise AnchorError(f"{path} is not importable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def is_anchor(value: object) -> bool:
    return isinstance(value, tuple) and len(value) == 2 and all(isinstance(s, str) for s in value)


def anchor_tables(module: ModuleType) -> dict[str, dict[str, tuple[str, str]]]:
    """Every module-level dict whose values are all `(old, new)` string pairs, by name."""
    return {
        name: table
        for name, table in vars(module).items()
        if isinstance(table, dict) and table and all(is_anchor(value) for value in table.values())
    }


def missing_anchors(directory: Path) -> tuple[list[str], int]:
    """The anchors (as `TABLE: name`) whose `old` text is not in DIR's `ref.py`, and the count checked."""
    tables = anchor_tables(load_mutants(directory))
    if not tables:
        raise AnchorError(f"{directory / MUTANTS} defines no table of (old, new) string pairs")
    source = (directory / REFERENCE_MODEL).read_text(encoding="utf-8")
    missing = [
        f"{table}: {name}"
        for table, entries in tables.items()
        for name, (old, _new) in entries.items()
        if old not in source
    ]
    return missing, sum(len(entries) for entries in tables.values())


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "directory",
        nargs="?",
        type=Path,
        default=REFERENCE,
        help=f"the directory holding {MUTANTS} and {REFERENCE_MODEL}",
    )
    args = parser.parse_args(argv)
    directory: Path = args.directory
    try:
        missing, checked = missing_anchors(directory)
    except AnchorError as err:
        print(f"FAIL {err}", file=sys.stderr)
        return 1
    for anchor in missing:
        print(
            f"FAIL mutation anchor missing or stale: {anchor} (its old text is not in "
            f"{directory / REFERENCE_MODEL}; the nightly's {MUTANTS} would fail before its first run)",
            file=sys.stderr,
        )
    if missing:
        return 1
    print(f"ok: {checked} mutation anchors in {directory / MUTANTS} found in {REFERENCE_MODEL}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
