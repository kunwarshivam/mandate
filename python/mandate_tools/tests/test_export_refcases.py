from pathlib import Path

import pytest

from mandate_tools.export_refcases import ExportError, export, load, render

ROOT = Path(__file__).resolve().parents[3]


def test_exponent_scalars_stay_strings():
    # journal.yaml's decimal vectors: YAML 1.2 loaders would read these as floats.
    assert load("[1e3, 1E+3, 1e-30, 8e28]") == ["1e3", "1E+3", "1e-30", "8e28"]


def test_quoted_decimals_and_plain_ints():
    assert load("{a: '0.020', b: 50, c: -3}") == {"a": "0.020", "b": 50, "c": -3}


@pytest.mark.parametrize(
    "text",
    [
        "{a: 1, a: 2}",
        "x: 1.5",
        "x: 2026-09-25",
        "x: yes",
        "x: on",
        "x: 0x1F",
        "x: 1_000",
    ],
)
def test_ambiguous_yaml_is_rejected(text):
    with pytest.raises(ExportError):
        load(text)


def test_true_false_allowed():
    assert load("{a: true, b: false}") == {"a": True, "b": False}


def test_render_is_deterministic_and_keeps_order():
    data = load("{b: 1, a: 'é'}")
    assert render(data) == render(data) == '{\n "b": 1,\n "a": "é"\n}\n'


def test_every_reference_file_exports(tmp_path):
    written = export(ROOT, tmp_path)
    assert [p.name for p in written] == ["trading-domain.json", "journal.json", "mandate.json"]
    assert all(p.stat().st_size > 0 for p in written)
