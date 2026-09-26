import json
from decimal import Decimal

import pytest

from research_spike.journal import GENESIS, Journal, JournalError, record_hash, sha256


def test_records_chain_by_sha256_and_persist(tmp_path):
    journal = Journal(tmp_path)
    first = journal.append("mark", {"closes": {"SPY": {"close": Decimal("663.10")}}})
    second = journal.append("thesis", {"status": "accepted"})
    assert (
        first["prev"] == GENESIS
        and second["prev"] == first["hash"]
        and (first["seq"], second["seq"]) == (1, 2)
    )
    third = journal.append("mark", {"f": 0.25})
    assert first["payload"]["closes"]["SPY"]["close"] == "663.10" and third["payload"]["f"] == "0.25"
    lines = (tmp_path / "journal.jsonl").read_text().splitlines()
    assert (
        sha256(
            json.dumps(
                {k: v for k, v in json.loads(lines[1]).items() if k != "hash"},
                sort_keys=True,
                separators=(",", ":"),
                ensure_ascii=False,
            ).encode()
        )
        == second["hash"]
    )
    reopened = Journal(tmp_path)
    reopened.verify()
    assert reopened.records() == [first, second, third] and reopened.last_hash() == third["hash"]
    assert record_hash(second) == second["hash"]


@pytest.mark.parametrize("edit", ["payload", "prev", "seq", "delete"])
def test_tampering_is_detected(tmp_path, edit):
    journal = Journal(tmp_path)
    for i in range(3):
        journal.append("mark", {"i": i})
    lines = (tmp_path / "journal.jsonl").read_text().splitlines()
    record = json.loads(lines[1])
    if edit == "delete":
        del lines[1]
    else:
        record[edit] = {"payload": {"i": 9}, "prev": GENESIS, "seq": 5}[edit]
        lines[1] = json.dumps(record)
    (tmp_path / "journal.jsonl").write_text("\n".join(lines) + "\n")
    with pytest.raises(JournalError, match="chain broken"):
        Journal(tmp_path).verify()


def test_unknown_kinds_and_unserializable_payloads_are_rejected(tmp_path):
    journal = Journal(tmp_path)
    with pytest.raises(JournalError):
        journal.append("order", {})
    with pytest.raises(TypeError):
        journal.append("mark", {"when": object()})
    assert journal.records() == []


def test_artifacts_are_stored_by_content_hash(tmp_path):
    journal = Journal(tmp_path)
    digest = journal.store_artifact("prompt text")
    assert (
        digest == sha256(b"prompt text")
        and (tmp_path / "artifacts" / f"{digest}.txt").read_text() == "prompt text"
    )
    assert journal.store_artifact("prompt text") == digest
