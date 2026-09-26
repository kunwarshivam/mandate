"""Append-only JSON Lines with a light SHA-256 chain, and content-addressed artifacts."""

import hashlib
import json
from datetime import UTC, datetime
from decimal import Decimal
from pathlib import Path

GENESIS = "0" * 64
KINDS = frozenset({"thesis", "order_submitted", "order_update", "fill", "mark", "score", "llm_call"})


class JournalError(Exception):
    pass


def canonical(value: object) -> bytes:
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, default=_plain
    ).encode()


def _plain(value: object) -> str:
    if isinstance(value, Decimal):
        return str(value)
    raise TypeError(f"{type(value).__name__} is not journalable")


def plain_payload(payload: dict) -> dict:
    """The payload as it will read back from the file: floats become decimal strings, never floats."""
    return json.loads(canonical(json.loads(canonical(payload), parse_float=Decimal)))


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def record_hash(record: dict) -> str:
    return sha256(canonical({key: value for key, value in record.items() if key != "hash"}))


class Journal:
    def __init__(self, directory: Path):
        self.directory = directory
        self.path = directory / "journal.jsonl"
        self.artifacts = directory / "artifacts"
        self.artifacts.mkdir(parents=True, exist_ok=True)
        self._records = self._load()

    def _load(self) -> list[dict]:
        if not self.path.exists():
            return []
        with self.path.open(encoding="utf-8") as handle:
            return [json.loads(line, parse_float=Decimal) for line in handle if line.strip()]

    def records(self, kind: str | None = None) -> list[dict]:
        return [r for r in self._records if kind is None or r["kind"] == kind]

    def last_hash(self) -> str:
        return self._records[-1]["hash"] if self._records else GENESIS

    def append(self, kind: str, payload: dict, at: datetime | None = None) -> dict:
        if kind not in KINDS:
            raise JournalError(f"unknown record kind {kind!r}")
        record = {
            "seq": len(self._records) + 1,
            "ts": (at or datetime.now(UTC)).isoformat(timespec="microseconds"),
            "kind": kind,
            "prev": self.last_hash(),
            "payload": plain_payload(payload),
        }
        record["hash"] = record_hash(record)
        with self.path.open("a", encoding="utf-8") as handle:
            handle.write(canonical(record).decode() + "\n")
        self._records.append(record)
        return record

    def verify(self) -> None:
        prev = GENESIS
        for index, record in enumerate(self._records, start=1):
            if record["seq"] != index or record["prev"] != prev or record_hash(record) != record["hash"]:
                raise JournalError(f"chain broken at seq {index}")
            prev = record["hash"]

    def store_artifact(self, text: str) -> str:
        digest = sha256(text.encode("utf-8"))
        target = self.artifacts / f"{digest}.txt"
        if not target.exists():
            target.write_text(text, encoding="utf-8")
        return digest
