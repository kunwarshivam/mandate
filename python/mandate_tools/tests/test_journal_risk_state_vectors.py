"""The risk-state journal vectors are what reference/journal/generate.py generates (DEC-403).

The generator's `--check` run also checks the `risk_state` section: journal spec §9.3's closed
schemas against every base draft and invalid draft, the JournaledFact each record maps to, and the
seeded mutants. This test shows a hand edit of that section is refused too.
"""

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
GENERATOR = ROOT / "reference/journal/generate.py"
VECTORS = ROOT / "docs/specs/reference-cases/journal.yaml"


def run(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(GENERATOR), *args], capture_output=True, text=True, check=False
    )


def test_the_generator_reports_the_risk_state_section():
    result = run("--check")
    assert result.returncode == 0, result.stderr
    assert "risk-state drafts" in result.stdout


def test_a_hand_edited_risk_state_vector_is_refused(tmp_path):
    text = VECTORS.read_text(encoding="utf-8")
    head, marker, section = text.partition("risk_state:\n")
    assert marker, "journal.yaml has no risk_state section"
    assert "allocation_change: '2500'" in section
    edited = tmp_path / "journal.yaml"
    edited.write_text(
        head + marker + section.replace("allocation_change: '2500'", "allocation_change: '2600'", 1),
        encoding="utf-8",
    )
    result = run("--check", "--vectors", str(edited))
    assert result.returncode == 1
    assert "differs" in result.stderr
