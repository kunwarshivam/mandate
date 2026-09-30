"""The agent-stream journal vectors are what reference/journal/generate.py generates (DEC-177).

`cargo xtask ci reference` runs only reference/mandate/, so this test is what holds the journal
generator's checks in CI: the version-3 self-test, the closed-schema validator against every chain
event and invalid draft, the independent recomputation, and the seeded mutants.
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


def test_committed_vectors_match_the_generator():
    result = run("--check")
    assert result.returncode == 0, result.stderr
    assert "mutants caught" in result.stdout


def test_a_hand_edited_vector_is_refused(tmp_path):
    text = VECTORS.read_text(encoding="utf-8")
    head, marker, section = text.partition("agent_stream:\n")
    assert marker, "journal.yaml has no agent_stream section"
    edited = tmp_path / "journal.yaml"
    edited.write_text(head + marker + section.replace("qty: '10'", "qty: '11'", 1), encoding="utf-8")
    result = run("--check", "--vectors", str(edited))
    assert result.returncode == 1
    assert "differs" in result.stderr
