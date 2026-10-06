"""The journal spec's canonical form (§4), field types (§9.1), envelope (§3), and chain helpers.

Shared by `generate.py`'s two generated sections: the agent stream's (§9.1, DEC-177) and the
control stream's (§9.2, DEC-261). Nothing here knows an event type.
"""

import calendar
import copy
import hashlib
import itertools
import re
from dataclasses import dataclass
from datetime import datetime
from decimal import Decimal, InvalidOperation

# --------------------------------------------------------------------------- spec §4: canonical form

KEY = re.compile(r"^[a-z][a-z0-9_]{0,63}\Z")
SHORT_ESCAPES = {'"': '\\"', "\\": "\\\\", "\b": "\\b", "\f": "\\f", "\n": "\\n", "\r": "\\r", "\t": "\\t"}
MAX_INT = 2**53 - 1


def canon_str(text: str) -> str:
    out = []
    for ch in text:
        code = ord(ch)
        if ch in SHORT_ESCAPES:
            out.append(SHORT_ESCAPES[ch])
        elif code < 0x20:
            out.append(f"\\u{code:04x}")
        elif 0xD800 <= code <= 0xDFFF:
            raise ValueError("lone surrogate (spec §4.3)")
        else:
            out.append(ch)
    return '"' + "".join(out) + '"'


def canon(value) -> str:
    """The canonical JSON text of a value tree (spec §4); bytes are its UTF-8 encoding."""
    match value:
        case None:
            return "null"
        case bool():
            return "true" if value else "false"
        case int():
            if not 0 <= value <= MAX_INT:
                raise ValueError(f"integer out of range: {value}")
            return str(value)
        case float():
            raise ValueError("floating-point numbers are rejected (spec §4.5)")
        case str():
            return canon_str(value)
        case list():
            return "[" + ",".join(canon(item) for item in value) + "]"
        case dict():
            for key in value:
                if not KEY.match(key):
                    raise ValueError(f"key outside the spec §4.1 charset: {key!r}")
            return "{" + ",".join(f"{canon_str(k)}:{canon(value[k])}" for k in sorted(value)) + "}"
    raise TypeError(f"not a JSON value: {value!r}")


def canon_bytes(value) -> bytes:
    return canon(value).encode("utf-8")


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


DECIMAL_INPUT = re.compile(r"^-?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?\Z")
DECIMAL_CANONICAL = re.compile(r"^-?(0|[1-9][0-9]*)(\.[0-9]*[1-9])?\Z")
DECIMAL_BOUND = Decimal("7.9e28")


def normalize_decimal(text: str) -> str | None:
    """The canonical form of a decimal input, or None if spec §4.6 rejects it."""
    if not DECIMAL_INPUT.match(text):
        return None
    try:
        number = Decimal(text)
    except InvalidOperation:
        return None
    if abs(number) >= DECIMAL_BOUND:
        return None
    plain = format(number, "f")
    if "." in plain:
        plain = plain.rstrip("0").rstrip(".")
    if plain in ("-0", ""):
        plain = "0"
    fraction = plain.partition(".")[2]
    if len(fraction) > 28:
        return None
    return plain


TIMESTAMP = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{9}Z\Z")


def is_timestamp(text: str) -> bool:
    if not TIMESTAMP.match(text):
        return False
    try:
        instant = datetime.strptime(text[:19], "%Y-%m-%dT%H:%M:%S")
    except ValueError:
        return False
    return 1970 <= instant.year <= 9999


def parse_instant(text: str) -> tuple[datetime, int]:
    return datetime.strptime(text[:19], "%Y-%m-%dT%H:%M:%S"), int(text[20:29])


def epoch_seconds(text: str) -> int:
    """The runtime's risk-clock form of an instant, which the invalid drafts show being refused."""
    return calendar.timegm(parse_instant(text)[0].timetuple())


IDENT = re.compile(r"^[A-Za-z0-9_-]+\Z")
ULID_ALPHABET = set("0123456789ABCDEFGHJKMNPQRSTVWXYZ")
DIGEST = re.compile(r"^sha256:[0-9a-f]{64}\Z")


def is_ulid(text: str) -> bool:
    return len(text) == 26 and set(text) <= ULID_ALPHABET and text[0] <= "7"

# --------------------------------------------------------------------------- field types (§9.1)


@dataclass(frozen=True)
class T:
    """One field type, mirroring the journal's payload type system."""

    kind: str
    inner: T | None = None
    options: tuple[str, ...] = ()
    fields: tuple[tuple[str, T], ...] = ()


STR, ID, ULID, DEC, INT, BOOL, TS, REF = (
    T(k) for k in ("str", "id", "ulid", "dec", "int", "bool", "ts", "ref")
)


def one_of(*options: str) -> T:
    return T("one_of", options=options)


def opt(inner: T) -> T:
    return T("nullable", inner=inner)


def list_of(inner: T) -> T:
    return T("list", inner=inner)


def rec(*fields: tuple[str, T]) -> T:
    return T("record", fields=fields)

STEP_UP = rec(("assertion_id", STR), ("authenticated_at", TS), ("method", STR))

CONFIG_REF_KINDS = (
    "fee_config",
    "trading_calendar",
    "settlement_calendar",
    "instrument_snapshot",
    "rule_set",
    "mandate_version",
    "model_version",
    "policy_set",
    "model_registry",
)
ENVELOPE = rec(
    ("envelope_version", INT),
    ("environment", one_of("paper", "live", "backtest")),
    ("event_id", ULID),
    ("stream_id", STR),
    ("event_type", STR),
    ("schema_version", INT),
    ("event_time", TS),
    ("clock_source", one_of("broker", "exchange", "local", "scheduler")),
    ("causation_id", opt(ULID)),
    ("correlation_id", opt(ULID)),
    (
        "actor",
        rec(
            ("kind", one_of("system", "agent", "user", "broker", "platform_operator")),
            ("id", STR),
            ("version", STR),
            ("build", opt(REF)),
        ),
    ),
    ("config_refs", T("object")),
    ("payload", T("object")),
    ("artifact_refs", list_of(REF)),
    ("pii_refs", list_of(STR)),
)
JOURNAL_FIELDS = ("seq", "prev_hash", "recorded_at")

@dataclass(frozen=True)
class Violation:
    rule: str
    reason: str
    path: str


def type_violations(ty: T, value, path: str, skip: frozenset[str]) -> list[Violation]:
    """Every type violation, in the order the journal reports them: extra members first (in key
    order), then the declared members in declaration order."""

    def bad(reason: str, rule: str = "types") -> list[Violation]:
        return [Violation(rule, reason, path)]

    kind = ty.kind
    if kind == "nullable":
        return [] if value is None else type_violations(ty.inner, value, path, skip)
    if kind == "record":
        if not isinstance(value, dict):
            return bad("schema")
        names = [name for name, _ in ty.fields]
        out = []
        if "record.extra" not in skip:
            out += [
                Violation("record.extra", "schema", f"{path}.{k}".lstrip("."))
                for k in sorted(value)
                if k not in names
            ]
        for name, inner in ty.fields:
            member = f"{path}.{name}".lstrip(".")
            if name not in value:
                if "record.missing" not in skip:
                    out.append(Violation("record.missing", "schema", member))
            else:
                out += type_violations(inner, value[name], member, skip)
        return out
    if kind == "list":
        if not isinstance(value, list):
            return bad("schema")
        return [
            v for i, item in enumerate(value) for v in type_violations(ty.inner, item, f"{path}[{i}]", skip)
        ]
    if kind == "object":
        return [] if isinstance(value, dict) else bad("schema")
    if kind == "int":
        ok = isinstance(value, int) and not isinstance(value, bool) and 0 <= value <= MAX_INT
        return [] if ok else bad("schema")
    if kind == "bool":
        return [] if isinstance(value, bool) else bad("schema")
    if not isinstance(value, str):
        return bad("schema")
    checks = {
        "str": lambda s: s != "" or "types.str_nonempty" in skip,
        "id": lambda s: bool(IDENT.match(s)),
        "ulid": is_ulid,
        "dec": lambda s: normalize_decimal(s) is not None,
        "ts": lambda s: is_timestamp(s) or "types.timestamp" in skip,
        "ref": lambda s: bool(DIGEST.match(s)),
        "one_of": lambda s: s in ty.options,
    }
    return [] if checks[kind](value) else bad("non_canonical")


def digest_strings(value) -> set[str]:
    match value:
        case str() if DIGEST.match(value):
            return {value}
        case list():
            return set().union(*(digest_strings(v) for v in value)) if value else set()
        case dict():
            return set().union(*(digest_strings(v) for v in value.values())) if value else set()
    return set()


def ascending(items: list) -> bool:
    """Strictly ascending, so a repeat is refused as well as a swap."""
    return all(a < b for a, b in itertools.pairwise(items))

def artifact_ref(obj: dict) -> str:
    return "sha256:" + sha256_hex(canon_bytes(obj))

def hash_chain(bodies: list[dict], genesis: str) -> list[dict]:
    entries, prev = [], genesis
    for body in bodies:
        body["prev_hash"] = prev
        text = canon(body)
        prev = sha256_hex(text.encode())
        entries.append(
            {
                "seq": body["seq"],
                "event_type": body["event_type"],
                "body": body,
                "canonical": text,
                "hash": prev,
            }
        )
    return entries


def rechain(entries: list[dict]) -> None:
    """Re-canonicalize and re-hash chain entries in place from the first body's `prev_hash`."""
    prev = entries[0]["body"]["prev_hash"]
    for entry in entries:
        entry["body"]["prev_hash"] = prev
        entry["canonical"] = canon(entry["body"])
        prev = entry["hash"] = sha256_hex(entry["canonical"].encode())


def draft_of(body: dict) -> dict:
    return {k: copy.deepcopy(v) for k, v in body.items() if k not in JOURNAL_FIELDS}


def apply_change(draft: dict, item: dict) -> None:
    *parents, last = item["path"].split(".")
    node = draft
    for name in parents:
        node = node[name]
    if item.get("delete"):
        del node[last]
    else:
        node[last] = copy.deepcopy(item["value"])


def change(path: str, value) -> dict:
    return {"path": path, "value": value}


def delete(path: str) -> dict:
    """The member is absent from the draft, which §4.2 distinguishes from `null`."""
    return {"path": path, "delete": True}
