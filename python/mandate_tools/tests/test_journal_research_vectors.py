"""The research journal vectors are what reference/journal/generate.py generates (DEC-413).

The generator's `--check` run also checks the `research` section: journal spec §9.4's closed
schemas for `ThesisProposed` and `ThesisRevised` against every base draft, invalid draft, and valid
draft, the oracles over the stored mandate and model, and the seeded mutants. This test shows a hand
edit of that section is refused too.
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


def test_the_generator_reports_the_research_section():
    result = run("--check")
    assert result.returncode == 0, result.stderr
    assert "research drafts" in result.stdout


def test_a_hand_edited_research_vector_is_refused(tmp_path):
    text = VECTORS.read_text(encoding="utf-8")
    head, marker, section = text.partition("\nresearch:\n")
    assert marker, "journal.yaml has no research section"
    assert "allowlist_version: 7" in section
    edited = tmp_path / "journal.yaml"
    edited.write_text(
        head + marker + section.replace("allowlist_version: 7", "allowlist_version: 8", 1),
        encoding="utf-8",
    )
    result = run("--check", "--vectors", str(edited))
    assert result.returncode == 1
    assert "differs" in result.stderr
