"""The control-stream journal vectors are what reference/journal/generate.py generates (DEC-261).

The generator's `--check` run (test_journal_agent_vectors.py) also checks the `control_stream`
section: the closed §9.2 schemas against every chain event, base draft, and invalid draft, the
JournaledFact each record maps to, and the seeded mutants. This test shows a hand edit of that
section is refused too.
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


def test_the_generator_reports_the_control_section():
    result = run("--check")
    assert result.returncode == 0, result.stderr
    assert "control-stream events" in result.stdout


def test_a_hand_edited_control_vector_is_refused(tmp_path):
    text = VECTORS.read_text(encoding="utf-8")
    head, marker, section = text.partition("control_stream:\n")
    assert marker, "journal.yaml has no control_stream section"
    assert "loss_added: '125.5'" in section
    edited = tmp_path / "journal.yaml"
    edited.write_text(
        head + marker + section.replace("loss_added: '125.5'", "loss_added: '125.6'", 1),
        encoding="utf-8",
    )
    result = run("--check", "--vectors", str(edited))
    assert result.returncode == 1
    assert "differs" in result.stderr
