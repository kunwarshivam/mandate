"""The anchor-only check of reference/mandate/mutants.py (#443 round 3).

`cargo xtask ci reference` runs `mandate_tools.mutation_anchors` per PR; these tests pin that it
passes on the committed reference, names an anchor whose `old` text `ref.py` no longer holds, and
refuses a `mutants.py` in which it finds no mutant table.
"""

import shutil
from pathlib import Path

import pytest

from mandate_tools import mutation_anchors

ROOT = Path(__file__).resolve().parents[3]
REFERENCE = ROOT / "reference/mandate"


def copy_reference(tmp_path: Path) -> Path:
    for name in (mutation_anchors.MUTANTS, mutation_anchors.REFERENCE_MODEL):
        shutil.copy(REFERENCE / name, tmp_path / name)
    return tmp_path


def test_every_committed_anchor_is_in_ref_py(capsys):
    missing, checked = mutation_anchors.missing_anchors(REFERENCE)
    assert missing == []
    assert checked >= 175, "the three tables held 175 anchors when this floor was set"
    assert mutation_anchors.main([str(REFERENCE)]) == 0
    assert capsys.readouterr().out.startswith(f"ok: {checked} mutation anchors in ")


def test_a_stale_anchor_is_named(tmp_path, capsys):
    directory = copy_reference(tmp_path)
    tables = mutation_anchors.anchor_tables(mutation_anchors.load_mutants(directory))
    assert set(tables) == {"MUTANTS", "TRIPWIRE_MUTANTS", "TRIM_MUTANTS", "UNASKED_MUTANTS"}
    name, (old, _new) = next(iter(tables["TRIM_MUTANTS"].items()))
    ref = directory / mutation_anchors.REFERENCE_MODEL
    text = ref.read_text(encoding="utf-8")
    assert old in text
    ref.write_text(text.replace(old, old.replace("sell", "selL", 1), 1), encoding="utf-8")

    missing, checked = mutation_anchors.missing_anchors(directory)
    assert checked == sum(len(table) for table in tables.values())
    assert mutation_anchors.main([str(directory)]) == 1
    err = capsys.readouterr().err
    assert f"FAIL mutation anchor missing or stale: TRIM_MUTANTS: {name} (" in err
    assert [m for m in missing if not m.startswith("TRIM_MUTANTS: ")] == [], (
        "every other table's anchors still occur in the edited ref.py"
    )
    assert f"TRIM_MUTANTS: {name}" in missing


def test_a_stale_unasked_anchor_is_named(tmp_path, capsys):
    directory = copy_reference(tmp_path)
    tables = mutation_anchors.anchor_tables(mutation_anchors.load_mutants(directory))
    name, (old, _new) = next(iter(tables["UNASKED_MUTANTS"].items()))
    ref = directory / mutation_anchors.REFERENCE_MODEL
    text = ref.read_text(encoding="utf-8")
    assert old in text
    ref.write_text(text.replace(old, old.replace("st", "sT", 1), 1), encoding="utf-8")

    missing, _checked = mutation_anchors.missing_anchors(directory)
    assert mutation_anchors.main([str(directory)]) == 1
    err = capsys.readouterr().err
    assert f"FAIL mutation anchor missing or stale: UNASKED_MUTANTS: {name} (" in err
    assert f"UNASKED_MUTANTS: {name}" in missing


def test_a_mutants_module_without_tables_is_refused(tmp_path, capsys):
    directory = copy_reference(tmp_path)
    (directory / mutation_anchors.MUTANTS).write_text("MUTANTS = {}\nOTHER = {'a': 'b'}\n", encoding="utf-8")
    with pytest.raises(mutation_anchors.AnchorError, match="defines no table"):
        mutation_anchors.missing_anchors(directory)
    assert mutation_anchors.main([str(directory)]) == 1
    assert "FAIL" in capsys.readouterr().err
