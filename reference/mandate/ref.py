"""Reference implementation of docs/specs/mandate.md (v0.6). Not production code."""
import copy, hashlib, json, pathlib
from datetime import datetime, timedelta, timezone
from zoneinfo import ZoneInfo
from decimal import Decimal as D, getcontext, ROUND_HALF_EVEN, ROUND_DOWN, ROUND_UP, ROUND_CEILING
import jsonschema

getcontext().prec = 60
ROOT = str(pathlib.Path(__file__).resolve().parents[2])
SCHEMA = json.load(open(f"{ROOT}/schemas/mandate.schema.json"))
PSCHEMA = json.load(open(f"{ROOT}/schemas/policy.schema.json"))
jsonschema.Draft202012Validator.check_schema(SCHEMA)
V = jsonschema.Draft202012Validator(SCHEMA)
PV = jsonschema.Draft202012Validator(PSCHEMA)
DEC_V = jsonschema.Draft202012Validator(SCHEMA["$defs"]["decimal"])
NY = ZoneInfo("America/New_York")
HARD = D("1.25")

# ------------------------------------------------------------------ numbers and time
def canon(o):
    return json.dumps(o, sort_keys=True, separators=(",", ":"), ensure_ascii=False)

def version(m):
    return "sha256:" + hashlib.sha256(canon(m).encode()).hexdigest()

def norm(x):
    x = D(x)
    if x == 0:
        return "0"
    s = format(x, "f")
    if "." in s:
        s = s.rstrip("0").rstrip(".")
    return s

def r12(x):
    return D(x).quantize(D("1e-12"), rounding=ROUND_HALF_EVEN)

def c12(x):
    return D(x).quantize(D("1e-12"), rounding=ROUND_CEILING)

def q12(x):
    return norm(r12(x))

def T(s):
    assert len(s) == 30 and s.endswith("Z"), s
    return datetime.strptime(s[:19], "%Y-%m-%dT%H:%M:%S").replace(tzinfo=timezone.utc)

def fmt(dt):
    return dt.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S") + ".000000000Z"

def regular_seconds(t0, t1):
    """Seconds of the regular session (09:30-16:00 America/New_York, weekdays) in [t0, t1)."""
    total = 0.0
    d = t0.astimezone(NY).date()
    end = t1.astimezone(NY).date()
    while d <= end:
        if d.weekday() < 5:
            o = datetime(d.year, d.month, d.day, 9, 30, tzinfo=NY)
            c = datetime(d.year, d.month, d.day, 16, 0, tzinfo=NY)
            lo, hi = max(o, t0), min(c, t1)
            if hi > lo:
                total += (hi - lo).total_seconds()
        d += timedelta(days=1)
    return total

def risk_day(instant):
    local = T(instant).astimezone(NY)
    day = local.date()
    start = datetime(day.year, day.month, day.day, tzinfo=NY)
    n = start + timedelta(days=1)
    nxt = datetime(n.year, n.month, n.day, tzinfo=NY)
    return {"risk_day": day.isoformat(), "starts_at": fmt(start), "ends_at": fmt(nxt),
            "length_s": int((nxt.astimezone(timezone.utc) - start.astimezone(timezone.utc)).total_seconds())}

# ------------------------------------------------------------------ JSON Patch (RFC 6902 subset)
def ptr(path):
    return [p.replace("~1", "/").replace("~0", "~") for p in path.lstrip("/").split("/")] if path else []

def get(doc, path):
    for p in ptr(path):
        doc = doc[int(p)] if isinstance(doc, list) else doc[p]
    return doc

def get_or_none(doc, path):
    """`get`, with `None` for a member one side of a version change does not hold (an optional field set or removed)."""
    try:
        return get(doc, path)
    except (KeyError, IndexError):
        return None

def apply_patch(doc, patch):
    doc = copy.deepcopy(doc)
    for op in patch:
        parts = ptr(op["path"])
        parent = doc
        for p in parts[:-1]:
            parent = parent[int(p)] if isinstance(parent, list) else parent[p]
        k = parts[-1]
        v = copy.deepcopy(op.get("value"))
        if op["op"] == "replace":
            if isinstance(parent, list):
                parent[int(k)] = v
            else:
                assert k in parent, op
                parent[k] = v
        elif op["op"] == "add":
            if isinstance(parent, list):
                parent.append(v) if k == "-" else parent.insert(int(k), v)
            else:
                parent[k] = v
        elif op["op"] == "remove":
            del parent[int(k) if isinstance(parent, list) else k]
        else:
            raise ValueError(op)
    return doc

# ------------------------------------------------------------------ conditions (§6.3)
FIELD_TYPES = {
    "purpose": "enum", "session": "enum", "asset_class": "enum", "instrument": "string",
    "order_usd": "decimal", "position_usd_after": "decimal", "gross_usd_after": "decimal",
    "bought_today_usd": "decimal", "daily_pnl_fraction": "decimal", "position_pnl_fraction": "decimal",
    "combined_score": "unit", "drawdown": "unit", "thesis_confidence": "unit",
    "unusual_input": "bool", "first_trade_in_instrument": "bool", "new_instrument": "bool",
}
ENUMS = {"purpose": {"open", "increase"}, "session": {"pre_market", "regular", "after_hours", "crypto"},
         "asset_class": {"us_equity", "crypto"}}
REDUCING = {"discretionary_exit", "risk_exit", "protective", "owner_exit"}
UNAVAILABLE_FIELDS = {"unusual_input"}

def comparisons(c, depth=1):
    if "all" in c or "any" in c:
        for x in c.get("all", c.get("any")):
            yield from comparisons(x, depth + 1)
    elif "not" in c:
        yield from comparisons(c["not"], depth + 1)
    else:
        yield c, depth

def is_dec(v):
    return isinstance(v, str) and DEC_V.is_valid(v)

def type_ok(field, op, value):
    t = FIELD_TYPES[field]
    if t == "bool":
        return op in ("eq", "ne") and isinstance(value, bool)
    if t in ("decimal", "unit"):
        if op in ("in", "not_in") or not is_dec(value):
            return False
        return t != "unit" or D(0) <= D(value) <= D(1)
    if op in ("gt", "gte", "lt", "lte"):
        return False
    if op in ("in", "not_in") and not isinstance(value, list):
        return False
    if op in ("eq", "ne") and not isinstance(value, str):
        return False
    vals = value if op in ("in", "not_in") else [value]
    if t == "enum":
        return all(isinstance(v, str) and v in ENUMS[field] for v in vals)
    return all(isinstance(v, str) for v in vals)

def cond(c, a):
    if "all" in c:
        return all(cond(x, a) for x in c["all"])
    if "any" in c:
        return any(cond(x, a) for x in c["any"])
    if "not" in c:
        return not cond(c["not"], a)
    f, op, v = c["field"], c["op"], c["value"]
    x = a[f]
    if FIELD_TYPES[f] in ("decimal", "unit"):
        x, v = D(x), D(v)
    return {"eq": lambda: x == v, "ne": lambda: x != v, "gt": lambda: x > v, "gte": lambda: x >= v,
            "lt": lambda: x < v, "lte": lambda: x <= v, "in": lambda: x in v, "not_in": lambda: x not in v}[op]()

def is_catch_all(c):
    return c.get("field") == "purpose" and c.get("op") == "in" and set(c.get("value", [])) >= {"open", "increase"}

# ------------------------------------------------------------------ semantic validation (§4)
SYSTEM_FIELDS = ["/mandate_schema_version", "/source_text_ref"]
PLATFORM_DEFAULTABLE = {   # path -> required value (None = any value)
    "/name": None, "/notifications": None, "/autonomy/approval/on_timeout": "skip",
    "/autonomy/approval/approvers": None, "/autonomy/default": "ask", "/autonomy/admission": "ask",
    "/universe/leveraged_etps_enabled": False, "/universe/leveraged_etp_disclosure_version": None,
    "/environment": "paper",
}
REVIEW_DEFAULT_DAYS = 90    # §7: the platform default for autonomy.review_by, counted from the validation date (DEC-188)
REVIEW_MAX_DAYS = 180       # V-046: a review date set or moved is at most this far after the validation date
OWNER_SOURCES = ("user_stated", "user_entered", "platform_proposed")
NEVER_PROPOSED = ["/universe/pinned_instruments", "/environment", "/connection_id"]

def covered(path, prefixes):
    return any(path == p or path.startswith(p + "/") for p in prefixes)

def sorted_unique(xs):
    return all(a < b for a, b in zip(xs, xs[1:]))

def valid_date(s):
    try:
        datetime.strptime(s, "%Y-%m-%d")
        return True
    except ValueError:
        return False

def days_after(day, n):
    return (datetime.strptime(day, "%Y-%m-%d") + timedelta(days=n)).strftime("%Y-%m-%d")

def review_errors(m, ctx):
    """V-015 and V-046 for `autonomy.review_by` (§3, §4.1, DEC-188, DEC-272). A date the version sets or moves lies in
    [validation date, validation date + 180 days]; one carried unchanged is not re-checked, so a lapsed date can stay
    lapsed through a reducing version; and once a version sets it, every later version does."""
    errs = set()
    rb = m["autonomy"].get("review_by")
    prev = ctx.get("previous_version")
    prev_rb = (prev.get("autonomy") or {}).get("review_by") if prev is not None else None
    if prev_rb is not None and rb is None:
        errs.add("V-046")
    if rb is None or rb == prev_rb:
        return errs
    if not valid_date(rb):
        errs.add("V-015")
        return errs
    vd = ctx["validation_date"]
    if not vd <= rb <= days_after(vd, REVIEW_MAX_DAYS):
        errs.add("V-046")
    return errs

TRIPWIRE_COUNTS = ("consecutive_losing_exits", "new_instruments")   # §6.7: counted metrics, whole thresholds
TRIPWIRE_MAX_COUNT = 1000                                            # V-044
TRIPWIRE_ACTIONS = {"end_delegations": 0, "exits_only": 1}           # §6.7, in increasing strictness; never `paused`

def tripwire_errors(m):
    """V-044 (§4.1, DEC-352): ids sorted and unique; a counted metric's threshold is a whole number from 1 to 1,000;
    `realized_loss_usd`'s is whole cents and at most the allocation."""
    tws = m["autonomy"].get("tripwires", [])
    if not sorted_unique([t["id"] for t in tws]):
        return {"V-044"}
    for t in tws:
        th = D(t["threshold"])
        if t["metric"] in TRIPWIRE_COUNTS:
            ok = th == th.to_integral_value() and 1 <= th <= TRIPWIRE_MAX_COUNT
        else:
            ok = -th.as_tuple().exponent <= 2 and th <= D(m["capital"]["allocation_usd"])
        if not ok:
            return {"V-044"}
    return set()

STOP_LIMIT_FIELD = {1: "crypto_stop_limit_offset", 2: "stop_limit_offset"}

def stop_limit_offset(p, m):
    """DEC-539 item 5: version 1's `crypto_stop_limit_offset` reads as version 2's `stop_limit_offset`."""
    return p[STOP_LIMIT_FIELD[m["mandate_schema_version"]]]

def upcast(m):
    """A version-1 document read as version 2 (DEC-539 item 5): only the protection field's name changes."""
    if m["mandate_schema_version"] == 2:
        return m
    out = copy.deepcopy(m)
    out["mandate_schema_version"] = 2
    out["protection"]["stop_limit_offset"] = out["protection"].pop("crypto_stop_limit_offset")
    return out

def stop_limit_protected(m, ctx):
    """V-008 and W-002 (DEC-539 items 2 and 3): whether any asset class the mandate may hold is protected by a
    stop-limit under its connection's profile. `stop_limit_asset_classes` is that explicit input. Absent or null (no
    profile, or an unknown broker), every allowed asset class counts as protected by a stop-limit: validation fails
    closed, as DEC-539's fixed table does for any other or unknown broker (spec §4)."""
    classes = (ctx or {}).get("stop_limit_asset_classes")
    if classes is None:
        return bool(m["universe"]["asset_classes"])
    return any(c in classes for c in m["universe"]["asset_classes"])

def worst_case(m, ctx=None):
    r, p = m["risk"], m["protection"]
    A = D(m["capital"]["allocation_usd"])
    pos = min(D(r["max_position_usd"]), D(r["max_position_fraction"]) * A)
    offset = D(stop_limit_offset(p, m) or 0) if stop_limit_protected(m, ctx) else D(0)
    stop = (D(p["stop_distance"]) + offset) if p["enabled"] else None
    return {"one_position_at_stop_usd": norm(pos * stop) if stop is not None else None,
            "daily_loss_budget_usd": norm(D(r["max_daily_loss"]) * A),
            "flatten_trigger_loss_usd": norm(D(r["max_drawdown"]) * A),
            "lifetime_floor_loss_usd": norm(D(m["capital"]["max_loss_from_allocation"]) * A)}

DELEGATION_MAX_SPAN_S = 30 * 86400

def delegation_errors(m, prev):
    """V-041, V-042, V-043 (§4.1, §6.5, DEC-181)."""
    errs = set()
    au = m["autonomy"]
    ds = au.get("delegations", [])
    if not ds:
        return errs
    ask_sources = {f"rule:{r['id']}" for r in au["rules"] if r["then"] == "ask"}
    if au["default"] == "ask":
        ask_sources.add("default")
    ids = [d["id"] for d in ds]
    if len(set(ids)) != len(ids):
        errs.add("V-041")
    for d in ds:
        span = (T(d["expires_at"]) - T(d["starts_at"])).total_seconds()
        if d["lifts"] not in ask_sources or not 0 < span <= DELEGATION_MAX_SPAN_S:
            errs.add("V-041")
        two = au["approval"]["two_approver_above_usd"]
        if not (D(d["max_order_usd"]) <= D(m["risk"]["max_order_usd"]) and D(d["max_order_usd"]) <= D(d["max_total_usd"])
                <= D(m["capital"]["allocation_usd"]) and (two is None or D(d["max_order_usd"]) <= D(two))):
            errs.add("V-043")
    if prev is not None and "autonomy" in prev:
        carried = {d["id"] for d in prev["autonomy"].get("delegations", [])} & set(ids)
        if carried and classify(without_delegations(prev, review_by_of=m), without_delegations(m))[0] == "risk_increasing":
            errs.add("V-042")
    return errs

def without_delegations(m, review_by_of=None):
    """`m` with no delegations. With `review_by_of`, `m` also takes that version's review date, so V-042 classifies a
    version with its review date left out: re-confirming carries the delegations over (DEC-188, DEC-273)."""
    out = copy.deepcopy(m)
    out["autonomy"].pop("delegations", None)
    if review_by_of is not None:
        out["autonomy"].pop("review_by", None)
        if "review_by" in review_by_of["autonomy"]:
            out["autonomy"]["review_by"] = review_by_of["autonomy"]["review_by"]
    return out

def reduces_previous(prev, current, m):
    """V-047's one exception (DEC-444): `m` is a new version that §9.2 classifies as risk-reducing against the agent's
    current version. `prev` is that version only if it is a whole schema-valid document whose canonical hash is
    `current`, the agent's current `mandate_version`, which the platform supplies and the requester never does
    (#570 round 1, B1): a predecessor nobody confirmed would let an increasing version pass. `classify`'s own result
    decides the rest. No previous version (a deployment), no current version, only an identity or part of a document,
    a document with a value the schema refuses, or one that hashes to anything else is not reducing: validation cannot
    classify against the agent's version (rule 3). The member test is `classify`'s precondition, so a partial document
    is refused without being classified; the schema test is the one that refuses a whole document with a bad value."""
    return (prev is not None and current is not None and set(SCHEMA["required"]) <= set(prev)
            and V.is_valid(prev) and version(prev) == current
            and classify(prev, m)[0] == "risk_reducing")

def semantic(m, ctx):
    errs, warns = set(), set()
    u = m["universe"]
    inst = u["pinned_instruments"]
    ids = [i["asset_id"] for i in inst]
    if ctx.get("connection_environment", m["environment"]) != m["environment"]:
        errs.add("V-001")
    if D(ctx["other_allocations_usd"]) + D(m["capital"]["allocation_usd"]) > D(ctx["account_equity_usd"]):
        errs.add("V-002")
    g = m["goal"]
    if g["type"] == "accumulate" and not (u["pinned"] and ids == [g["instrument"]] and m["behavior"]["research"] is None):
        errs.add("V-003")
    if m["universe"]["leveraged_etps_enabled"]:
        dv = m["universe"]["leveraged_etp_disclosure_version"]
        if dv is None or dv not in ctx.get("disclosures_accepted", []):
            errs.add("V-005")
    groups = ctx.get("instrument_groups", {})
    claimed = {groups.get(a, a) for a in ctx.get("claimed_by_other_agents", [])}
    if any(groups.get(a, a) in claimed for a in ids):
        errs.add("V-006")
    reg = ctx.get("registry")
    for sm in m["behavior"]["signal_models"]:
        r0 = reg.get(sm["id"]) if reg is not None else None
        if reg is not None and (r0 is None or r0["version"] != sm["version"] or r0["content_hash"] != sm["content_hash"]
                                or sorted(r0["params"]) != [p["key"] for p in sm["params"]]):
            errs.add("V-007")
    p = m["protection"]
    if p["enabled"]:
        if stop_limit_protected(m, ctx) and stop_limit_offset(p, m) is None:
            errs.add("V-008")
    elif p["stop_distance"] is not None or p["take_profit_distance"] is not None or stop_limit_offset(p, m) is not None:
        errs.add("V-008")
    sms = m["behavior"]["signal_models"]
    rule_ids = [x["id"] for x in m["autonomy"]["rules"]]
    if not (sorted_unique(ids) and sorted_unique([s["id"] for s in sms]) and len(set(rule_ids)) == len(rule_ids)
            and all(sorted_unique([q["key"] for q in s["params"]]) for s in sms)
            and sorted_unique(m["behavior"]["cadence"]["event_sources"])
            and sorted_unique(m["notifications"]["channels"]) and sorted_unique(u["asset_classes"])
            and sorted_unique(m["autonomy"]["approval"]["approvers"])):
        errs.add("V-009")
    r = m["risk"]
    lad = r["drawdown_ladder"]
    ats = [D(x["at"]) for x in lad]
    sev = {"scale_sizes": 0, "exits_only": 1, "flatten_and_pause": 2}
    if any(b <= a for a, b in zip(ats, ats[1:])) or any(sev[b["action"]] < sev[a["action"]] for a, b in zip(lad, lad[1:])):
        errs.add("V-010")
    for x in lad:
        if (x["action"] == "scale_sizes") != (x["factor"] is not None):
            errs.add("V-010")
    if lad[-1]["action"] != "flatten_and_pause" or D(lad[-1]["at"]) != D(r["max_drawdown"]) \
            or sum(1 for x in lad if x["action"] == "flatten_and_pause") != 1:
        errs.add("V-011")
    if D(r["hysteresis"]) >= ats[0]:
        errs.add("V-012")
    if not (D(r["max_order_usd"]) <= D(r["max_position_usd"]) <= D(r["max_gross_exposure_usd"]) <= D(m["capital"]["allocation_usd"])):
        errs.add("V-013")
    if D(m["capital"]["max_loss_from_allocation"]) < D(r["max_drawdown"]):
        errs.add("V-014")
    if g.get("end_date") is not None and not valid_date(g["end_date"]):
        errs.add("V-015")
    q = m["notifications"]["quiet_hours"]
    if q is not None and q["start"] == q["end"]:
        errs.add("V-016")
    for rule in m["autonomy"]["rules"] + m["autonomy"].get("delegations", []):
        for c, depth in comparisons(rule["when"]):
            if depth > 4:
                errs.add("V-017")
            if c["field"] in UNAVAILABLE_FIELDS:
                errs.add("V-018")
            if not type_ok(c["field"], c["op"], c["value"]):
                errs.add("V-023")
    prov = ctx.get("provenance", {})
    multi = ctx.get("workspace_users", 1) > 1
    for path, pv in prov.items():
        if covered(path, SYSTEM_FIELDS):
            continue
        if pv["source"] == "platform_default":
            allowed = next((k for k in PLATFORM_DEFAULTABLE if path == k or path.startswith(k + "/")), None)
            if path == "/autonomy/review_by":
                if m["autonomy"].get("review_by") != days_after(ctx["validation_date"], REVIEW_DEFAULT_DAYS):
                    errs.add("V-020")
            elif allowed is None or (PLATFORM_DEFAULTABLE[allowed] is not None and get(m, allowed) != PLATFORM_DEFAULTABLE[allowed]) \
                    or (allowed == "/autonomy/approval/approvers" and multi):
                errs.add("V-020")
        elif pv["source"] not in OWNER_SOURCES or not pv["confirmed"]:
            errs.add("V-020")
        if pv["source"] == "platform_proposed" and covered(path, NEVER_PROPOSED):
            errs.add("V-038")
    autos = ["/autonomy/default"] if m["autonomy"]["default"] == "auto" else []
    autos += ["/autonomy/admission"] if m["autonomy"]["admission"] == "auto" else []
    autos += [f"/autonomy/rules/{i}" for i, x in enumerate(m["autonomy"]["rules"]) if x["then"] == "auto"]
    autos += [f"/autonomy/delegations/{i}" for i in range(len(m["autonomy"].get("delegations", [])))]
    for a in autos:
        pv = prov.get(a, {"source": "user_entered", "confirmed": True})
        if pv["source"] != "user_entered" or not pv["confirmed"]:
            errs.add("V-022")
    ap = m["autonomy"]["approval"]
    n_users = ctx.get("approver_users", 1)
    if n_users < 1 or (ap["two_approver_above_usd"] is not None and n_users < 2):
        errs.add("V-024")
    if ctx.get("independent_approval_required", False) and ctx.get("workspace_users", 1) < 2:
        if not reduces_previous(ctx.get("previous_version"), ctx.get("current_mandate_version"), m):
            errs.add("V-047")
    if g.get("end_date") is not None and valid_date(g["end_date"]) and g["end_date"] < ctx["validation_date"]:
        errs.add("V-030")
    prev = ctx.get("previous_version")
    if prev is not None and (prev["environment"] != m["environment"] or prev["connection_id"] != m["connection_id"]
                             or m["mandate_schema_version"] < prev.get("mandate_schema_version", m["mandate_schema_version"])):
        errs.add("V-031")
    if r["scale_action"] == "trim_to_target" and g["type"] == "accumulate":
        errs.add("V-033")
    admitting = [s for s in sms if s["admits_instruments"]]
    if len(admitting) > 1 or (admitting and not admitting[0]["id"].startswith("llm.")) \
            or (len(admitting) == 1) != (m["behavior"]["research"] is not None):
        errs.add("V-036")
    if u["pinned"] and admitting:
        errs.add("V-037")
    if u["pinned"] != (len(inst) > 0):
        errs.add("V-034")
    if u["max_instruments"] < len(inst):
        errs.add("V-035")
    if any(i["asset_class"] not in u["asset_classes"] for i in inst):
        errs.add("V-039")
    if sum(-D(x["factor"]).as_tuple().exponent for x in lad if x["action"] == "scale_sizes" and x["factor"] is not None) > 12:
        errs.add("V-040")
    errs |= delegation_errors(m, prev)
    errs |= review_errors(m, ctx)
    errs |= tripwire_errors(m)
    carry = D(ctx.get("connection_loss_carry_usd", "0"))
    if carry >= D(m["capital"]["max_loss_from_allocation"]) * D(m["capital"]["allocation_usd"]):
        errs.add("V-032")
    if ctx.get("eligibility_failures"):
        warns.add("W-001")
    wc = worst_case(m, ctx)
    if wc["one_position_at_stop_usd"] is not None and D(wc["one_position_at_stop_usd"]) > D(wc["daily_loss_budget_usd"]):
        warns.add("W-002")
    if not p["enabled"]:
        warns.add("W-003")
    if m["autonomy"]["admission"] == "auto" and admitting:
        warns.add("W-006")
    rules = m["autonomy"]["rules"]
    for i, rule in enumerate(rules):
        if is_catch_all(rule["when"]) and i < len(rules) - 1:
            warns.add("W-005")
    return sorted(errs), sorted(warns)

# ------------------------------------------------------------------ policy hierarchy (§4.3)
MAND_PATH = {
    "allocation_usd": "/capital/allocation_usd", "max_loss_from_allocation": "/capital/max_loss_from_allocation",
    "max_position_usd": "/risk/max_position_usd", "max_position_fraction": "/risk/max_position_fraction",
    "max_gross_exposure_usd": "/risk/max_gross_exposure_usd", "max_order_usd": "/risk/max_order_usd",
    "max_orders_per_day": "/risk/max_orders_per_day", "max_daily_loss": "/risk/max_daily_loss",
    "max_drawdown": "/risk/max_drawdown", "breach_confirm_s": "/risk/breach_confirm_s",
    "exit_threshold": "/behavior/sizing/exit_threshold", "entry_threshold": "/behavior/sizing/entry_threshold",
    "rebalance_band": "/behavior/sizing/rebalance_band", "hysteresis": "/risk/hysteresis",
    "cadence_interval_s": "/behavior/cadence/interval_s", "approval_timeout_s": "/autonomy/approval/timeout_s",
    "reentry_cooldown_s": "/risk/reentry_cooldown_s", "daily_breach_min_s": "/risk/daily_breach_min_s",
    "scale_lift_after_s": "/risk/scale_lift_after_s", "max_instruments": "/universe/max_instruments",
}
P_MAX = ["allocation_usd", "max_loss_from_allocation", "max_position_usd", "max_position_fraction",
         "max_gross_exposure_usd", "max_order_usd", "max_orders_per_day", "max_daily_loss", "max_drawdown",
         "breach_confirm_s", "max_output_age_s", "exit_threshold", "stop_distance_max", "exits_only_at_max",
         "two_approver_above_usd", "max_instruments", "research_weight", "research_cost_cap_usd_per_day",
         "max_revisions_per_lineage"]
P_MIN = ["entry_threshold", "rebalance_band", "hysteresis", "cadence_interval_s", "approval_timeout_s",
         "reentry_cooldown_s", "daily_breach_min_s", "scale_lift_after_s", "research_interval_s",
         "stagger_window_s"]
P_ENABLE = ["leveraged_etps_allowed", "auto_allowed", "research_agent_allowed", "admission_auto_allowed"]
P_REQUIRE = ["protection_required", "independent_approval_required"]
P_SET = ["asset_classes", "signal_model_types", "goal_types", "channels", "environments"]
RETAIL_PROFILE = {"auto_allowed": True, "signal_model_types": ["llm", "quant"], "leveraged_etps_allowed": False,
                  "protection_required": True, "approval_timeout_s": 120, "max_loss_from_allocation": "0.2",
                  "research_agent_allowed": False, "environments": ["paper"]}
INTERNAL_RESEARCH_PROFILE = {"research_agent_allowed": True, "admission_auto_allowed": False,
                             "max_revisions_per_lineage": 3, "environments": ["paper"]}
PLATFORM_BASE = {"max_loss_from_allocation": "0.5", "breach_confirm_s": 300, "max_instruments": 20,
                 "stagger_window_s": 900}

def mandate_policy_values(m):
    v = {k: get(m, p) for k, p in MAND_PATH.items()}
    v["max_output_age_s"] = max(s["max_output_age_s"] for s in m["behavior"]["signal_models"])
    v["stop_distance_max"] = m["protection"]["stop_distance"]
    v["exits_only_at_max"] = [x["at"] for x in m["risk"]["drawdown_ladder"] if x["action"] != "scale_sizes"][0]
    v["two_approver_above_usd"] = m["autonomy"]["approval"]["two_approver_above_usd"]
    v["leveraged_etps_allowed"] = m["universe"]["leveraged_etps_enabled"]
    v["auto_allowed"] = m["autonomy"]["default"] == "auto" or any(r["then"] == "auto" for r in m["autonomy"]["rules"]) \
        or bool(m["autonomy"].get("delegations"))
    v["protection_required"] = m["protection"]["enabled"]
    v["asset_classes"] = m["universe"]["asset_classes"]
    v["signal_model_types"] = sorted({s["id"].split(".")[0] for s in m["behavior"]["signal_models"]})
    res = m["behavior"]["research"]
    admitting = [s for s in m["behavior"]["signal_models"] if s["admits_instruments"]]
    v["research_agent_allowed"] = bool(admitting)
    v["admission_auto_allowed"] = m["autonomy"]["admission"] == "auto"
    v["environments"] = [m["environment"]]
    if admitting:
        v["research_weight"] = admitting[0]["weight"]
    if res is not None:
        v["research_interval_s"] = res["interval_s"]
        v["research_cost_cap_usd_per_day"] = res["cost_cap_usd_per_day"]
        v["max_revisions_per_lineage"] = res["max_revisions_per_lineage"]
    v["goal_types"] = [m["goal"]["type"]]
    v["channels"] = m["notifications"]["channels"]
    return v

def violates(key, child, parent):
    if key in ("two_approver_above_usd", "stop_distance_max"):
        return child is None or D(child) > D(parent)
    if child is None:
        return False
    if key in P_MAX:
        return D(child) > D(parent)
    if key in P_MIN:
        return D(child) < D(parent)
    if key in P_ENABLE:
        return child is True and parent is False
    if key in P_REQUIRE:
        return parent is True and child is not True
    if key in P_SET:
        return not set(child) <= set(parent)
    raise KeyError(key)

def policy_check(m, levels):
    chain = levels + [("mandate", mandate_policy_values(m))]
    out = []
    for i in range(1, len(chain)):
        cname, cvals = chain[i]
        for key, cv in cvals.items():
            for pname, pvals in reversed(chain[:i]):
                if key in pvals and violates(key, cv, pvals[key]):
                    out.append({"key": key, "level": cname, "value": cv, "limit_level": pname, "limit": pvals[key]})
                    break
    return out

# ------------------------------------------------------------------ risk state (§5)
SEVERITY = {"normal": 0, "exits_only": 1, "paused": 2, "stopped": 3}
LATCHES = ("daily_loss", "drawdown_exits_only", "drawdown_flatten", "lifetime_floor")

class Confirm:
    """Time-in-breach confirmation (§5.6)."""
    def __init__(self):
        self.acc = 0.0
        self.false_run = 0.0
        self.prev = False

    def update(self, breached, dt, need):
        if self.prev:
            self.acc += dt
        else:
            self.false_run += dt
        if self.false_run >= need and not self.prev:
            self.acc = 0.0
        if breached:
            self.false_run = 0.0
        self.prev = breached
        return breached and self.acc >= need

    @property
    def pending(self):
        return self.prev or self.acc > 0

class RiskState:
    def __init__(self, m, qty, avg_cost, asset_class, start, inherited_loss="0", mark_max_age_s=120):
        r = m["risk"]
        self.m = m
        self.A = D(m["capital"]["allocation_usd"])
        self.C = self.A
        self.L = D(inherited_loss)
        self.f = D(m["capital"]["max_loss_from_allocation"])
        self.qty = D(qty)
        self.B = D(qty) * D(avg_cost)
        self.realized = D(0)
        self.mark = D(avg_cost)
        self.cls = asset_class
        self.lad = r["drawdown_ladder"]
        self.hyst = D(r["hysteresis"])
        self.mdl = D(r["max_daily_loss"])
        self.daily_action = r["daily_loss_action"]
        self.need = r["breach_confirm_s"]
        self.daily_min = r["daily_breach_min_s"]
        self.lift_after = r["scale_lift_after_s"]
        self.max_age = mark_max_age_s
        self.E = self.equity()
        self.H = self.E
        self.E0 = self.E
        self.scale = {i: {"active": False, "lift_acc": 0.0, "lift_prev": False} for i, x in enumerate(self.lad) if x["action"] == "scale_sizes"}
        self.reset_queue = []
        self.conf = {}
        self.latched = {}
        self.daily = None
        self.restrictions = {}
        self.inst_restrictions = set()
        self.mode = "normal"
        self.t = T(start)
        self.session = "regular" if asset_class == "us_equity" else "crypto"
        self.mark_age = 0.0
        self.disarmed = False
        self.hard_first = {}      # severe limit -> risk-clock time the 1.25x level was first seen on a quote
        self.rollover = None      # previous day's pending daily breach, still confirming (§5.4)
        self.net_contributed = self.A  # dollars allocated in, minus dollars withdrawn (§5.7)
        self.retired = False
        self.working = D(0)
        self.tw = None            # §6.7: the tripwire fold, when the mandate has tripwires; a fired one is latched (MI-7)
        self._tw_input = None
        if (m.get("autonomy") or {}).get("tripwires"):
            self.tw = Tripwires()
            self.tw.step({"event": "MandateVersionApplied", "mandate": m})
            if self.qty > 0:
                self.tw.book[TW_AGENT_INSTRUMENT] = (self.qty, self.B)
                self.tw.ever_filled.add(TW_AGENT_INSTRUMENT)

    def equity(self):
        return self.A + self.realized + self.qty * self.mark - self.B

    def eff_mode(self):
        return max(self.restrictions.values(), key=lambda x: SEVERITY[x], default="normal")

    def counts(self, session):
        return self.cls == "crypto" or session == "regular"

    def conditions(self, E=None, H=None, E0=None, C=None):
        E = self.E if E is None else E
        H = self.H if H is None else H
        E0 = self.E0 if E0 is None else E0
        C = self.C if C is None else C
        out = {}
        for i, rung in enumerate(self.lad):
            at = D(rung["at"])
            out[f"drawdown_ladder[{i}]"] = (H - E >= at * H, H - E >= HARD * at * H)
        out["max_daily_loss"] = (E - E0 <= -self.mdl * E0, E - E0 <= -HARD * self.mdl * E0)
        out["lifetime_floor"] = (E <= C * (1 - self.f) + self.L, E <= C * (1 - HARD * self.f) + self.L)
        return out

    def hard_wait(self):
        return min(self.need, 10)

    def severe(self, key):
        """Every confirmable limit escalates on a second quote (DEC-63); kept as a hook for the text of §5.6."""
        return True

    def decide(self, key, hit, hard, t, dt, quote, ev):
        """Returns the trigger reason (None = no trigger) for a confirmable limit (§5.6, DEC-63)."""
        c = self.conf.setdefault(key, Confirm())
        confirmed = c.update(hit, dt, self.need)
        if confirmed:
            self._clear_hard(key, ev, quiet=True)
            return "confirmed"
        if not hard:
            if quote:
                self._clear_hard(key, ev)
            return None
        if not self.severe(key):
            return "hard_trigger"
        if key not in self.hard_first:
            if quote:
                self.hard_first[key] = t
                self.restrictions["hard_breach"] = "exits_only"
                ev.append({"type": "RiskLimitTriggered", "limit": key, "action": "exits_only", "reason": "hard_breach_pending"})
            return None
        if quote and (t - self.hard_first[key]).total_seconds() >= self.hard_wait():
            self._clear_hard(key, ev, quiet=True)
            return "hard_trigger"
        return None

    def _clear_hard(self, key, ev, quiet=False):
        if key in self.hard_first:
            del self.hard_first[key]
            if not quiet:
                ev.append({"type": "RiskLimitLifted", "limit": key, "reason": "hard_breach_cleared"})
            if not self.hard_first:
                self.restrictions.pop("hard_breach", None)

    def latch(self, key, action, ev, reason=None):
        e = {"type": "RiskLimitTriggered", "limit": key, "action": action}
        if reason:
            e["reason"] = reason
        ev.append(e)

    def step(self, s):
        if s["event"] == "allocation_change" and T(s["at"]) > self.t:
            first = self.step({"event": "clock", "at": s["at"], "session": s.get("session", self.session)})
            second = self.step(s)
            second["journal"] = first["journal"] + second["journal"]
            return second
        ev, err, extra = [], None, {}
        t = T(s["at"])
        dt = (t - self.t).total_seconds()
        assert dt >= 0, "risk clock is monotone"
        risk_dt = dt if self.cls == "crypto" else regular_seconds(self.t, t)
        self.t = t
        self.session = s.get("session", self.session)
        kind = s["event"]
        self._reset_now = False
        self._tw_input = None
        self._inst_reason = {}
        inst_before = set(self.inst_restrictions)
        # ---- stale-mark timer (advances before this input is applied)
        if self.qty > 0:
            self.mark_age += risk_dt
            if self.mark_age >= self.max_age:
                self.inst_restrictions.add("stale_mark")
        # ---- apply the input
        if kind == "mark":
            if self.cls == "us_equity" and s["session"] != "regular":
                pass
            elif not s.get("sane", True):
                if self.qty > 0:
                    self.inst_restrictions.add("stale_mark")
            else:
                self.mark = D(s["bid"])
                self.mark_age = 0.0
                self.inst_restrictions.discard("stale_mark")
        elif kind == "fill":
            q, px = D(s["qty"]), D(s["price"])
            if self.tw is not None:
                self._tw_input = {"event": "FillApplied", "instrument": TW_AGENT_INSTRUMENT, "side": s["side"],
                                  "qty": s["qty"], "price": s["price"], "fees": "0"}
            if s["side"] == "sell":
                red = self.B if q == self.qty else r12(self.B * q / self.qty)
                self.realized += q * px - red
                self.B -= red
                self.qty -= q
            else:
                self.B += q * px
                self.qty += q
        elif kind == "risk_day_started":
            E = self.equity()
            dconf = self.conf.get("max_daily_loss")
            if self.daily is None and dconf and dconf.pending:
                self.rollover = {"E0": self.E0, "conf": dconf, "hard_first": self.hard_first.pop("max_daily_loss", None)}
            self.conf.pop("max_daily_loss", None)
            self.E0 = E
            ev.append({"type": "RiskDayStarted", "day_start_equity": norm(self.E0)})
            if self.tw is not None:
                self._tw_input = {"event": "RiskDayStarted"}
            if self.daily:
                self.daily["day_started"] = True
        elif kind == "owner_acknowledged" and s["restriction"].startswith("tripwire:") and self.tw is not None:
            self._tw_input = {k: v for k, v in s.items() if k in ACK_FIELDS} | {"event": "OwnerAcknowledged",
                                                                               "tripwire": s["restriction"][len("tripwire:"):]}
        elif kind == "owner_acknowledged":
            err = self._ack(s["restriction"], ev, t)
        elif kind == "allocation_change":
            err = self._allocation(D(s["delta_usd"]), ev)
        elif kind == "clock":
            pass
        elif kind == "universe_changed":
            if s["change"] == "removed":
                self.inst_restrictions.add("removed_instrument")
            else:
                self.inst_restrictions.discard("removed_instrument")
            self._inst_reason["removed_instrument"] = s["reason"]
            ev.append({"type": "UniverseChanged", "instrument": s["instrument"], "change": s["change"],
                       "reason": s["reason"]})
        elif kind == "floor_loosened":
            err = self._loosen_floor(s, ev, t)
        elif kind == "agent_stopped":
            self._retire(s.get("reason", "owner_stop"), ev)
        elif kind == "goal_complete" and self.m["goal"]["type"] == "profit_stop":
            ev.append({"type": "GoalCompleted", "then": "discretionary_exit_all_then_retire"})
            self.restrictions["goal_complete"] = "exits_only"
        elif kind == "goal_complete":
            oc = self.m["goal"]["on_complete"]
            ev.append({"type": "GoalCompleted", "on_complete": oc})
            if oc == "release":
                ev.append({"type": "PositionReleased", "qty": norm(self.qty)})
                self._retire("goal_complete", ev)
            else:
                self.restrictions["goal_complete"] = "exits_only"
                if oc == "disarm_ladder":
                    self.disarmed = True
        # ---- evaluate (§5.2 order)
        quote = kind in ("mark", "fill") and (kind != "mark" or s.get("sane", True)) and not (kind == "mark" and self.cls == "us_equity" and s["session"] != "regular")
        self.E = self.equity()
        self.H = max(self.H, self.E)
        H, E = self.H, self.E
        conds = self.conditions()
        for i, rung in enumerate(self.lad):
            if self.disarmed:
                break
            key = f"drawdown_ladder[{i}]"
            hit, hard = conds[key]
            at = D(rung["at"])
            if rung["action"] == "scale_sizes":
                st = self.scale[i]
                if st["lift_prev"] and not self._reset_now:
                    st["lift_acc"] += risk_dt
                if hit:
                    st["lift_acc"] = 0.0
                    if not st["active"]:
                        st["active"] = True
                        if self.reset_queue:
                            self.reset_queue = sorted(self.reset_queue + [i], key=lambda j: D(self.lad[j]["at"]), reverse=True)
                        ev.append({"type": "RiskLimitTriggered", "limit": key, "action": "scale_sizes"})
                elif st["active"]:
                    can_lift = H - E < (at - self.hyst) * H and (not self.reset_queue or self.reset_queue[0] == i)
                    if not can_lift:
                        st["lift_acc"] = 0.0
                    elif st["lift_acc"] >= self.lift_after:
                        st["active"] = False
                        st["lift_acc"] = 0.0
                        if self.reset_queue and self.reset_queue[0] == i:
                            self.reset_queue.pop(0)
                        ev.append({"type": "RiskLimitLifted", "limit": key, "action": "scale_sizes"})
            elif key not in self.latched:
                why = self.decide(key, hit, hard, t, dt, quote, ev)
                if why:
                    self.conf.pop(key, None)
                    self.latched[key] = True
                    self.latch(key, rung["action"], ev, reason=None if why == "confirmed" else why)
                    if rung["action"] == "exits_only":
                        self.restrictions["drawdown_exits_only"] = "exits_only"
                    else:
                        self.restrictions["drawdown_flatten"] = "paused"
                        ev.append({"type": "KillSwitchActivated", "scope": "agent", "initiator": key})
        for i, st in self.scale.items():
            at_i = D(self.lad[i]["at"])
            st["lift_prev"] = (st["active"] and not self.disarmed and H - E < (at_i - self.hyst) * H
                               and (not self.reset_queue or self.reset_queue[0] == i))
            if not st["lift_prev"]:
                st["lift_acc"] = 0.0 if not st["active"] or H - E >= (at_i - self.hyst) * H else st["lift_acc"]
        hit, hard = conds["max_daily_loss"]
        if self.rollover and not self.disarmed:
            ro = self.rollover
            ohit = E - ro["E0"] <= -self.mdl * ro["E0"]
            ohard = E - ro["E0"] <= -HARD * self.mdl * ro["E0"]
            if ro["conf"].update(ohit, dt, self.need) or (ohard and not self.severe("max_daily_loss")):
                self.rollover = None
                if self.daily is None:
                    self.daily = {"at": t, "day_started": False, "acked": False}
                    self.latch("max_daily_loss", self.daily_action, ev, reason="resolved_at_rollover")
                    self._apply_daily(ev)
            elif not ro["conf"].pending:
                self.rollover = None
        if self.disarmed:
            pass
        elif self.daily is None:
            why = self.decide("max_daily_loss", hit, hard, t, dt, quote, ev)
            if why:
                self.conf.pop("max_daily_loss", None)
                self.daily = {"at": t, "day_started": False, "acked": False}
                self.latch("max_daily_loss", self.daily_action, ev, reason=None if why == "confirmed" else why)
                self._apply_daily(ev)
        else:
            if self.daily["day_started"]:
                c = self.conf.setdefault("max_daily_loss", Confirm())
                if c.update(hit, dt, self.need) or (hard and quote):
                    self.conf.pop("max_daily_loss", None)
                    self.daily = {"at": t, "day_started": False, "acked": self.daily["acked"] and self.daily_action == "exits_only"}
                    self.latch("max_daily_loss", self.daily_action, ev, reason="new_day_breach")
            if self.daily["day_started"] and (t - self.daily["at"]).total_seconds() >= self.daily_min \
                    and (self.daily_action == "exits_only" or self.daily["acked"]):
                self.daily = None
                self.conf.pop("max_daily_loss", None)
                self.restrictions.pop("daily_loss", None)
                ev.append({"type": "RiskLimitLifted", "limit": "max_daily_loss"})
        if "lifetime_floor" not in self.latched:
            hit, hard = conds["lifetime_floor"]
            why = self.decide("lifetime_floor", hit, hard, t, dt, quote, ev)
            if why:
                self.conf.pop("lifetime_floor", None)
                self.latched["lifetime_floor"] = True
                self.restrictions["lifetime_floor"] = "paused"
                self.latch("lifetime_floor", "flatten_and_pause", ev, reason=None if why == "confirmed" else why)
                ev.append({"type": "KillSwitchActivated", "scope": "agent", "initiator": "lifetime_floor"})
        if self._tw_input is not None:
            self._tripwires(ev)
        # profit_stop confirmation (§3.1): time in breach, no hard trigger
        g = self.m["goal"]
        if g["type"] == "profit_stop" and "goal_complete" not in self.restrictions and not self.retired:
            c = self.conf.setdefault("profit_stop", Confirm())
            if c.update(E - self.C >= D(g["profit_level"]) * self.C, dt, self.need):
                self.conf.pop("profit_stop", None)
                self.restrictions["goal_complete"] = "exits_only"
                ev.append({"type": "GoalCompleted", "reason": "profit_stop_reached", "then": "discretionary_exit_all_then_retire"})
        if self.qty == 0:
            self.inst_restrictions.discard("stale_mark")
        for r in sorted(inst_before ^ self.inst_restrictions):
            active = r in self.inst_restrictions
            ev.append({"type": "InstrumentRestrictionChanged", "restriction": r,
                       "reason": self._inst_reason.get(r, "no_sane_mark" if active else "sane_mark"),
                       "active": active})
        new = self.eff_mode()
        if new != self.mode:
            ev.append({"type": "AgentModeApplied", "from": self.mode, "to": new})
            self.mode = new
        out = self.snapshot()
        out["journal"] = ev
        if err:
            out["error"] = err
        out.update(extra)
        return out

    def _tripwires(self, ev):
        """§5.2 and §6.7: tripwires are evaluated after the lifetime floor. A fired one is a latched limit, so MI-7
        rejects allocation increases while it holds, and one that holds `exits_only` adds restriction `tripwire`."""
        ev.extend(self.tw.step(self._tw_input))
        held = self.tw.snapshot()
        for k in [k for k in self.latched if k.startswith("tripwire:") and k[len("tripwire:"):] not in held["fired"]]:
            del self.latched[k]
        for tid in held["fired"]:
            self.latched[f"tripwire:{tid}"] = True
        if held["restriction"] is not None:
            self.restrictions["tripwire"] = held["restriction"]
        else:
            self.restrictions.pop("tripwire", None)

    def _retire(self, reason, ev):
        """Retirement (§5.7, MI-14): the connection carries the net dollar loss max(0, N - E), whether the
        owner stopped the agent or a completed goal released its positions (DEC-270)."""
        ev.append({"type": "AgentStopped", "reason": reason,
                   "loss_carry_usd": norm(max(D(0), self.net_contributed - self.equity()))})
        self.restrictions["retired"] = "stopped"
        self.retired = True

    def _apply_daily(self, ev):
        if self.daily_action == "exits_only":
            self.restrictions["daily_loss"] = "exits_only"
        else:
            self.restrictions["daily_loss"] = "paused"
            ev.append({"type": "KillSwitchActivated", "scope": "agent", "initiator": "max_daily_loss"})

    def _ack(self, target, ev, t):
        if target == "lifetime_floor":
            return "not_acknowledgeable"
        if target == "drawdown_ladder":
            if not any(k.startswith("drawdown_ladder") for k in self.latched):
                return "nothing_to_acknowledge"
            if "drawdown_flatten" in self.restrictions and self.qty > 0:
                return "flatten_in_progress"
            self.E = self.equity()
            ev.append({"type": "HighWaterMarkReset", "from": norm(self.H), "to": norm(self.E)})
            self.H = self.E
            for k in sorted(self.latched):
                if k.startswith("drawdown_ladder"):
                    del self.latched[k]
                    ev.append({"type": "RiskLimitLifted", "limit": k, "reason": "owner_acknowledged"})
            for k in list(self.conf):
                if k.startswith("drawdown_ladder"):
                    del self.conf[k]
            for k in list(self.hard_first):
                if k.startswith("drawdown_ladder"):
                    self._clear_hard(k, ev)
            self.restrictions.pop("drawdown_exits_only", None)
            self.restrictions.pop("drawdown_flatten", None)
            for i, st in self.scale.items():
                if not st["active"]:
                    st["active"] = True
                    ev.append({"type": "RiskLimitTriggered", "limit": f"drawdown_ladder[{i}]", "action": "scale_sizes", "reason": "after_reset"})
                st["lift_acc"] = 0.0
            self.reset_queue = sorted(self.scale, key=lambda i: D(self.lad[i]["at"]), reverse=True)
            self._reset_now = True
            return None
        if target == "daily_loss":
            if not self.daily or self.daily_action != "flatten_and_pause" or self.daily["acked"]:
                return "nothing_to_acknowledge"
            if self.qty > 0:
                return "flatten_in_progress"
            self.daily["acked"] = True
            self.restrictions["daily_loss"] = "exits_only"
            return None
        return "unknown_restriction"

    def _allocation(self, d, ev):
        E = self.equity()
        E1 = E + d
        gross = self.qty * self.mark + self.working
        err = None
        if d > 0 and (self.latched or self.daily is not None):
            err = "increase_blocked_while_latched"
        elif E1 <= 0 or E1 < gross:
            err = "equity_below_exposure"
        else:
            H1, E01, C1, L1 = c12(self.H * E1 / E), c12(self.E0 * E1 / E), c12(self.C * E1 / E), c12(self.L * E1 / E)
            before = self.conditions(E, self.H, self.E0, self.C)
            L0, self.L = self.L, L1
            after = self.conditions(E1, H1, E01, C1)
            self.L = L0
            if any(after[k][j] and not before[k][j] for k in after for j in (0, 1)):
                err = "would_trigger_limit"
        if err:
            ev.append({"type": "MandateVersionApplied", "result": "rejected", "reason": err})
            return err
        self.H, self.E0, self.C, self.L = H1, E01, C1, L1
        self.A += d
        self.net_contributed += d
        ev.append({"type": "MandateVersionApplied", "result": "applied", "allocation_change": norm(d)})
        return None

    def _loosen_floor(self, s, ev, t):
        f1 = D(s["new_max_loss_from_allocation"])
        err = None
        if f1 <= self.f:
            err = "not_loosening"
        elif "lifetime_floor" in self.latched and not s.get("independent_approval", False):
            d1 = T(risk_day(s["confirmed_at"])["ends_at"])
            d2 = T(risk_day(fmt(d1))["ends_at"])
            if t < d2:
                err = "waiting_period"
        if not err and "lifetime_floor" in self.latched and not (self.equity() > self.C * (1 - f1) + self.L):
            err = "still_below_new_floor"
        if err:
            ev.append({"type": "MandateVersionApplied", "result": "rejected", "reason": err})
            return err
        self.f = f1
        ev.append({"type": "MandateVersionApplied", "result": "applied", "max_loss_from_allocation": norm(f1)})
        if "lifetime_floor" in self.latched:
            del self.latched["lifetime_floor"]
            self.restrictions.pop("lifetime_floor", None)
            self.conf.pop("lifetime_floor", None)
            ev.append({"type": "RiskLimitLifted", "limit": "lifetime_floor", "reason": "version_loosened"})
        return None

    def snapshot(self):
        factor = D(1)
        for i in sorted(self.scale):
            if self.scale[i]["active"]:
                factor *= D(self.lad[i]["factor"])
        return {"agent_equity": norm(self.E), "high_water_mark": norm(self.H), "drawdown": q12((self.H - self.E) / self.H),
                "day_start_equity": norm(self.E0), "daily_pnl": norm(self.E - self.E0),
                "daily_pnl_fraction": q12((self.E - self.E0) / self.E0), "capital_base": norm(self.C),
                "size_factor": norm(factor), "restrictions": sorted(self.restrictions), "agent_mode": self.mode,
                "instrument_restrictions": sorted(self.inst_restrictions),
                "pending": sorted(k for k, c in self.conf.items() if c.pending), "net_contributed": norm(self.net_contributed)}

# ------------------------------------------------------------------ gate: mandate limits (§5.3)
def gate(m, st, prop):
    r = m["risk"]
    if prop["purpose"] in REDUCING:
        return {"verdict": "allow", "reason": None}
    E = D(st["agent_equity"])
    mv = {k: D(v) for k, v in st["positions_mv"].items()}
    working = st.get("working_opening_orders", [])
    order = D(prop["qty"]) * D(prop["limit_price"])
    inst = prop["instrument"]
    if inst not in st["working_universe"]:
        return {"verdict": "deny", "reason": "not_in_working_universe", "computed": {"instrument": inst}}
    cap = min(D(r["max_position_usd"]), D(r["max_position_fraction"]) * E)
    inst_total = mv.get(inst, D(0)) + sum(D(w["max_cost"]) for w in working if w["instrument"] == inst) + order
    comp = {"instrument_total": norm(inst_total), "cap": norm(cap)}
    if inst_total > cap:
        return {"verdict": "deny", "reason": "concentration_limit", "computed": comp}
    if order > D(r["max_order_usd"]):
        return {"verdict": "deny", "reason": "max_order_size", "computed": {"order_usd": norm(order)}}
    groups = st.get("instrument_groups", {})
    g = groups.get(inst, inst)
    for other, last in st.get("last_exit_fill_at", {}).items():
        if groups.get(other, other) == g and (T(st["now"]) - T(last)).total_seconds() < r["reentry_cooldown_s"]:
            return {"verdict": "deny", "reason": "reentry_cooldown", "computed": {"last_exit_fill_at": last, "instrument": other}}
    if st.get("orders_today", 0) + 1 > r["max_orders_per_day"]:
        return {"verdict": "deny", "reason": "max_orders_per_day", "computed": {"orders_today": st["orders_today"]}}
    gross = sum(abs(v) for v in mv.values()) + sum(D(w["max_cost"]) for w in working) + order
    glim = min(D(r["max_gross_exposure_usd"]), E)
    comp.update({"gross": norm(gross), "gross_limit": norm(glim)})
    if gross > glim:
        return {"verdict": "deny", "reason": "gross_exposure_limit", "computed": comp}
    return {"verdict": "allow", "reason": None, "computed": comp}

# ------------------------------------------------------------------ order decision (§6.1, trading §7.4, §9)
def order_decision(m, st, prop, mode, inst_restrictions, session, in_close_window, owner_confirmed_bid=False,
                   asset_class="us_equity", kill_switch=False):
    """Composition used by the fuzz for MI-1: agent mode, instrument restrictions, session, close window, limits."""
    p = prop["purpose"]
    if mode == "stopped" or (mode == "paused" and not (p == "protective" or (p in ("risk_exit", "owner_exit") and kill_switch))):
        return {"verdict": "hold", "reason": f"agent_{mode}"}
    if mode == "exits_only" and p in ("open", "increase"):
        return {"verdict": "deny", "reason": "agent_exits_only"}
    if p in ("open", "increase") and inst_restrictions:
        return {"verdict": "deny", "reason": sorted(inst_restrictions)[0]}
    equity = asset_class == "us_equity"
    if p == "discretionary_exit" and equity and session != "regular":
        return {"verdict": "defer", "reason": "discretionary_exit_regular_session_only"}
    if p == "owner_exit" and equity and session != "regular" and not owner_confirmed_bid:
        return {"verdict": "defer", "reason": "owner_confirmation_required"}
    if p in ("open", "increase") and equity and in_close_window:
        return {"verdict": "deny", "reason": "close_window"}
    return gate(m, st, prop)

# ------------------------------------------------------------------ autonomy (§6)
STRICT = {"auto": 0, "ask": 1, "deny": 2}

def delegation_suspended(st):
    """§6.5 condition 5 (MI-28): any sign of trouble suspends every delegation, a fired tripwire included (§6.7, MI-31)."""
    return (st.get("mode", "normal") != "normal" or st.get("rungs_active", 0) > 0 or st.get("limit_pending", False)
            or st.get("limit_latched", False) or st.get("kill_switch", False)
            or st.get("tripwire_fired", False))

def delegation_lift(m, a, res, st):
    """§6.2 step 4a: the first live delegation naming the ask's source turns it into auto. `st` carries the risk
    clock (`now`), usage per delegation id counted from journaled decisions, and the suspension state."""
    if st is None or res["decision"] != "ask" or delegation_suspended(st):
        return None
    now = T(st["now"])
    for d in m["autonomy"].get("delegations", []):
        used = st.get("usage", {}).get(d["id"], {"orders": 0, "total_usd": "0"})
        if (d["lifts"] == res["by"] and T(d["starts_at"]) <= now and now < T(d["expires_at"])
                and cond(d["when"], a) and D(a["order_usd"]) <= D(d["max_order_usd"])
                and used["orders"] < d["max_orders"]
                and D(used["total_usd"]) + D(a["order_usd"]) <= D(d["max_total_usd"])):
            return d["id"]
    return None

def review_passed(m, st):
    """§6.2 step 5b (MI-32): the review date has passed once the risk day of the risk clock is after it, so the date
    itself is the last risk day autonomy stands. With no risk clock to judge by, it has passed (rule 3)."""
    rb = m["autonomy"].get("review_by")
    if rb is None:
        return False
    if st is None or "now" not in st:
        return True
    return risk_day(st["now"])["risk_day"] > rb

def autonomy(m, a, st=None):
    au = m["autonomy"]
    if a["purpose"] in REDUCING:
        return {"decision": "auto", "by": "builtin_risk_reducing"}
    res = None
    for rule in au["rules"]:
        if cond(rule["when"], a):
            res = {"decision": rule["then"], "by": f"rule:{rule['id']}"}
            break
    if res is None:
        res = {"decision": au["default"], "by": "default"}
    lifted_by = delegation_lift(m, a, res, st)
    if lifted_by is not None:
        res = {"decision": "auto", "by": f"delegation:{lifted_by}", "delegation_id": lifted_by, "lifted": res["by"]}
    if a.get("new_instrument", False) and STRICT[au["admission"]] > STRICT[res["decision"]]:
        res = {"decision": au["admission"], "by": "admission_ceiling"}
    if a.get("requested_by", "agent") == "client" and STRICT[res["decision"]] < STRICT["ask"]:
        res = {"decision": "ask", "by": "client_ceiling"}
    if review_passed(m, st) and STRICT[res["decision"]] < STRICT["ask"]:
        res = {"decision": "ask", "by": "review_ceiling"}
    if res["decision"] == "ask":
        t = au["approval"]["two_approver_above_usd"]
        res["approvers_required"] = 2 if t is not None and D(a["order_usd"]) > D(t) else 1
        res["on_timeout"] = "skip"
    return res

# ------------------------------------------------------------------ tripwires (§6.7; MI-31; DEC-187, DEC-350, DEC-351)
TRIPWIRE_ALERT_TEXT = "tripwire_fired"
TW_AGENT_INSTRUMENT = "agent_instrument"   # the risk state's one instrument, as the tripwire fold names it
ACK_FIELDS = ("at", "step_up", "user", "requester", "independent_approval_required", "independent_now")

def fill_net_realized(book, f):
    """The fill's net realized P&L (trading spec §8.1): its gross realized less its own fees, a buy's gross being 0.
    Long only (no short sales in v1). `book` maps an instrument to [Q, B] and is updated in place."""
    q, b = book.get(f["instrument"], (D(0), D(0)))
    qty, px = D(f["qty"]), D(f["price"])
    if f["side"] == "buy":
        book[f["instrument"]] = (q + qty, b + qty * px)
        gross = D(0)
    else:
        assert 0 < qty <= q, "no short sales"
        if qty == q:
            gross, book[f["instrument"]] = qty * px - b, (D(0), D(0))
        else:
            u = b - r12(b * qty / q)
            r = b - u if u >= 0 else b
            gross, book[f["instrument"]] = qty * px - r, (q - qty, b - r)
    return gross - D(f["fees"])

class Tripwires:
    """The tripwire state the executor folds from the agent's account stream (§6.7). Inputs, in `seq` order, by
    `event`: MandateVersionApplied (`mandate` is now in effect), FillApplied or LateFillApplied (the agent's fill:
    instrument, side, qty, price, and its own fees), RiskDayStarted, and OwnerAcknowledged (naming `tripwire`, its
    `step_up` judged at the risk clock `at` when processed, §6.1). Independence is required when the policy
    required it when the lift was requested (`independent_approval_required`) or requires it at processing
    (`independent_now`), the stricter governing; then the acknowledgment must name both the acknowledging `user`
    and the `requester`, and they must differ (§5.8), or it is refused `not_independent`. Each tripwire counts its
    metric over the inputs after the one that armed it: the
    version that began its run under that id and metric, or the acknowledgment that last lifted it."""

    def __init__(self, environment="paper"):
        self.environment = environment
        self.m = None
        self.book = {}
        self.ever_filled = set()
        self.counters = {}
        self.fired = {}
        self.used = set()

    def current(self):
        return {t["id"]: t for t in (self.m["autonomy"].get("tripwires", []) if self.m is not None else [])}

    def value(self, tid):
        c, metric = self.counters[tid], self.current()[tid]["metric"]
        if metric == "consecutive_losing_exits":
            return D(c["streak"])
        if metric == "new_instruments":
            return D(c["new"])
        return max(D(0), -c["day_net"])

    def effective_action(self, tid):
        """A fired tripwire holds the stricter of the action it fired with and its action in the version in effect."""
        a, now = self.fired[tid], self.current().get(tid)
        if now is not None and TRIPWIRE_ACTIONS[now["action"]] > TRIPWIRE_ACTIONS[a]:
            return now["action"]
        return a

    @staticmethod
    def armed():
        return {"streak": 0, "new": 0, "day_net": D(0)}

    def step(self, inp):
        ev = []
        kind = inp["event"]
        if kind == "MandateVersionApplied":
            before = self.current()
            self.m = inp["mandate"]
            for tid, t in self.current().items():
                if tid not in before or before[tid]["metric"] != t["metric"]:
                    self.counters[tid] = self.armed()
            self.counters = {k: v for k, v in self.counters.items() if k in self.current()}
        elif kind in ("FillApplied", "LateFillApplied"):
            net = fill_net_realized(self.book, inp)
            first = inp["instrument"] not in self.ever_filled
            self.ever_filled.add(inp["instrument"])
            for c in self.counters.values():
                if inp["side"] == "sell":
                    c["streak"] = c["streak"] + 1 if net < 0 else 0
                c["new"] += 1 if first else 0
                c["day_net"] += net
        elif kind == "RiskDayStarted":
            for c in self.counters.values():
                c["day_net"] = D(0)
        elif kind == "OwnerAcknowledged":
            verdict = owner_command("acknowledge", inp.get("step_up"), None, inp["at"], self.environment, self.used)
            if inp.get("step_up") is not None and isinstance(inp["step_up"], dict) and "assertion" in inp["step_up"]:
                self.used.add(inp["step_up"]["assertion"])
            tid = inp["tripwire"]
            if verdict["result"] == "refused":
                ev.append({"type": "OwnerCommandRefused", "command": "acknowledge", "reason": verdict["reason"]})
            elif ((inp.get("independent_approval_required", False) or inp.get("independent_now", False))
                  and (inp.get("user") is None or inp.get("requester") is None or inp.get("user") == inp.get("requester"))):
                ev.append({"type": "OwnerCommandRefused", "command": "acknowledge", "reason": "not_independent"})
            elif tid in self.fired:
                del self.fired[tid]
                ev.append({"type": "RiskLimitLifted", "limit": f"tripwire:{tid}", "reason": "owner_acknowledged"})
                if tid in self.current():
                    self.counters[tid] = self.armed()
        else:
            raise ValueError(kind)
        for tid, t in sorted(self.current().items()):
            if tid in self.fired:
                continue
            v = self.value(tid)
            if v >= D(t["threshold"]):
                self.fired[tid] = t["action"]
                ev.append({"type": "RiskLimitTriggered", "limit": f"tripwire:{tid}", "action": t["action"],
                           "reason": "tripwire_condition", "metric": t["metric"], "threshold": t["threshold"], "value": norm(v)})
                ev.append({"type": "OwnerAlertSent", "subject": f"RiskLimitTriggered:{len(ev) - 1}", "text": TRIPWIRE_ALERT_TEXT})
        return ev

    def snapshot(self):
        held = {tid: self.effective_action(tid) for tid in sorted(self.fired)}
        return {"fired": held,
                "restriction": "exits_only" if "exits_only" in held.values() else None,
                "delegations_suspended": bool(held),
                "metrics": {tid: norm(self.value(tid)) for tid in sorted(self.counters)}}

def tripwire_run(steps, environment="paper"):
    """Folds `steps` and returns, per step, the account-stream events it emitted and the state after it."""
    tw = Tripwires(environment)
    return [{"journal": tw.step(s), "state": tw.snapshot()} for s in steps]

def tripwire_autonomy_state(snapshot, now, mode="normal"):
    """The §6.5 state a decision reads while tripwires are in `snapshot`: a fired tripwire suspends every delegation,
    and an `exits_only` one adds its restriction to the effective mode (§5.9)."""
    held = mode if SEVERITY[mode] >= SEVERITY["exits_only"] or snapshot["restriction"] is None else snapshot["restriction"]
    return {"now": now, "usage": {}, "mode": held, "tripwire_fired": snapshot["delegations_suspended"]}

# ------------------------------------------------------------------ order builder (§8.3)
def trunc(x, inc):
    return (x / inc).to_integral_value(rounding=ROUND_DOWN) * inc

def ceil_inc(x, inc):
    return (x / inc).to_integral_value(rounding=ROUND_UP) * inc

def combine(m, inp):
    beh = m["behavior"]
    now = T(inp["now"])
    models = {s["id"]: s for s in beh["signal_models"]}
    W = sum(D(s["weight"]) for s in beh["signal_models"])
    latest = {}
    for i, o in enumerate(inp["outputs"]):
        sm = models.get(o["model_id"])
        if sm is None or o["model_version"] != sm["version"] or o["content_hash"] != sm["content_hash"]:
            continue
        a, e = T(o["as_of"]), T(o["expires_at"])
        if not (a <= now < e) or (now - a).total_seconds() > sm["max_output_age_s"]:
            continue
        prev = latest.get(o["model_id"])
        if prev is None or (a, i) > (T(prev[1]["as_of"]), prev[0]):
            latest[o["model_id"]] = (i, o)
    used = sorted(latest)
    fresh = sum(D(models[k]["weight"]) * D(latest[k][1]["conviction"]) * D(latest[k][1]["confidence"]) for k in used)
    missing = sum(D(sm["weight"]) for sm in beh["signal_models"] if sm["id"] not in latest)
    c_exit = r12(fresh / W)
    c_buy = r12((fresh - missing) / W)
    s = r12(sum(D(models[k]["weight"]) * D(latest[k][1]["confidence"]) for k in used) / W)
    return used, c_exit, c_buy, s

def builder(m, inp):
    r, beh = m["risk"], m["behavior"]
    sz = beh["sizing"]
    E = D(inp["agent_equity"])
    cap = min(D(r["max_position_usd"]), D(r["max_position_fraction"]) * E)
    factor = D(inp.get("size_factor", "1"))
    qty = D(inp["position_qty"])
    bid, ask = D(inp["quote"]["bid"]), D(inp["quote"]["ask"])
    inc = D(inp["qty_increment"])
    mv = qty * bid
    working = sum(D(w["max_cost"]) for w in inp.get("working_opening_orders", []))
    out = {"cap": norm(cap), "current_mv": norm(mv)}
    # risk engine first: trim_to_target (§5.5, DEC-65)
    band_usd = D(beh["sizing"]["rebalance_band"]) * cap
    # The agent's own non-protective sells already resting in the instrument are a trim in progress: the
    # trim is what they leave of the excess and of the position, and the minimum is judged on that
    # (DEC-399 item 7). When they cover the excess, no trim is due. They come off the excess before it
    # is rounded up on the instrument's quantity grid (DEC-445 item 2), and a remainder beside them
    # that is off the grid and is not the whole position is truncated onto it: the largest quantity on
    # the grid at or below what they leave unsold. Zero, or below the minimum, is withheld by the
    # guards below (DEC-423 item 2).
    on_sale = D(inp["open_sell_qty"]) if r["scale_action"] == "trim_to_target" else D(0)
    sell = D(0)
    if r["scale_action"] == "trim_to_target" and factor < 1 and mv - factor * cap >= band_usd:
        owed = max(D(0), mv - factor * cap - on_sale * bid)
        rounded = ceil_inc(owed / bid, inc)
        unsold = max(D(0), qty - on_sale)
        if rounded <= unsold:
            sell = rounded
        elif unsold == qty:
            sell = qty
        else:
            sell = trunc(unsold, inc)
    if sell > 0:
        guards = []
        if inp.get("scale_active_s", 0) < r["breach_confirm_s"]:
            guards.append("rung_not_confirmed")
        if inp.get("holding", False):
            guards.append("holding")
        if inp["asset_class"] == "us_equity" and inp.get("session", "regular") != "regular":
            guards.append("regular_session_only")
        # §5.5's minimum is the instrument's minimum order size (trading spec §5.3 rule 2; DEC-399 item 5),
        # except for a trim of the whole position, a full close rule 2 exempts (DEC-423). Beside a resting
        # sell the trim is never the whole position, so the exemption does not reach it.
        if sell < D(inp["min_order_size"]) and sell != qty:
            guards.append("below_minimum_order")
        if not guards:
            out.update({"action": "sell", "purpose": "risk_exit", "origin": "risk_engine", "reason": "trim_to_target",
                        "qty": norm(sell), "limit_price": norm(bid), "order_usd": norm(sell * bid),
                        "autonomy": {"decision": "auto", "by": "builtin_risk_reducing"}})
            return out
        out["trim_withheld"] = guards
    used, c, c_buy, s = combine(m, inp)
    out.update({"outputs_used": used, "combined_conviction": norm(c), "buy_conviction": norm(c_buy), "combined_score": norm(s)})
    goal = m["goal"]
    if not used:
        out.update({"action": "hold", "reason": "no_fresh_outputs"})
        return out
    entry, exitt = D(sz["entry_threshold"]), D(sz["exit_threshold"])
    equity_cls = inp["asset_class"] == "us_equity"
    if c <= -exitt:
        if goal["type"] == "accumulate":
            out.update({"action": "hold", "reason": "discretionary_exits_disabled"})
            return out
        if qty == 0:
            out.update({"action": "hold", "reason": "no_position"})
            return out
        out.update({"action": "sell", "purpose": "discretionary_exit", "qty": norm(qty), "limit_price": norm(bid),
                    "order_usd": norm(qty * bid)})
        sess = inp.get("session", "regular")
        if equity_cls and sess != "regular":
            out["gate_dry_run"] = {"verdict": "defer", "reason": "discretionary_exit_regular_session_only"}
            out["autonomy"] = {"decision": "deferred", "by": "gate_dry_run"}
        elif equity_cls and inp.get("in_close_window", False):
            out["order_type"] = "marketable_limit"
            out["gate_dry_run"] = {"verdict": "allow", "reason": None}
            out["autonomy"] = autonomy(m, {"purpose": "discretionary_exit"})
        else:
            out["gate_dry_run"] = {"verdict": "allow", "reason": None}
            out["autonomy"] = autonomy(m, {"purpose": "discretionary_exit"})
        return out
    if c_buy < entry:
        out.update({"action": "hold", "reason": "between_thresholds"})
        return out
    Tv = c_buy * cap * factor
    out["target_value"] = q12(Tv)
    delta = Tv - mv - working
    out["delta"] = q12(delta)
    band = D(sz["rebalance_band"]) * cap
    if delta <= 0:
        out.update({"action": "hold", "reason": "at_or_above_target"})
        return out
    if delta < band:
        out.update({"action": "hold", "reason": "within_rebalance_band"})
        return out
    gross_now = D(inp.get("gross_usd", mv + working))
    budget = min(delta, D(r["max_order_usd"]), cap - mv - working, min(D(r["max_gross_exposure_usd"]), E) - gross_now)
    clips = ["limits"] if budget < delta else []
    n = trunc(budget / ask, inc)
    if n * ask < band:
        out.update({"action": "hold", "reason": "below_band_after_clipping", "clipped_by": clips})
        return out
    if goal["type"] == "accumulate":
        rc, ra = D(inp.get("fee_rate_cash", "0")), D(inp.get("fee_rate_asset", "0"))
        a_unit, b_unit = ask * (1 + rc), 1 - ra
        remaining = D(goal["target_qty"]) - qty
        cands = [trunc(remaining / b_unit, inc)]
        spent = D(inp.get("goal_spent_usd", "0"))
        cands.append(trunc((D(goal["max_spend_usd"]) - spent) / a_unit, inc))
        g = min(cands)
        cb = D(inp.get("cost_basis_usd", "0"))
        if goal["max_avg_price"] is not None:
            mx = D(goal["max_avg_price"])
            den = a_unit - mx * b_unit
            if den > 0:
                g = min(g, trunc((mx * qty - cb) / den, inc))
        if g < n:
            clips.append("goal")
            n = max(g, D(0))
        if goal["max_avg_price"] is not None and n > 0:
            mx = D(goal["max_avg_price"])
            if (cb + n * a_unit) > mx * (qty + n * b_unit):
                out.update({"action": "hold", "reason": "would_exceed_max_avg_price", "clipped_by": clips})
                return out
    if n <= 0 or n * ask < D(inp["min_order_usd"]):
        out.update({"action": "hold", "reason": "below_minimum_after_clipping", "clipped_by": clips})
        return out
    out.update({"action": "buy", "purpose": "open" if qty == 0 else "increase", "qty": norm(n),
                "limit_price": norm(ask), "order_usd": norm(n * ask), "clipped_by": clips})
    st = dict(inp["gate_state"], now=inp["now"])
    dry = gate(m, st, {"instrument": inp["instrument"], "purpose": out["purpose"], "qty": out["qty"], "limit_price": out["limit_price"]})
    out["gate_dry_run"] = {"verdict": dry["verdict"], "reason": dry["reason"]}
    if dry["verdict"] == "deny":
        out["autonomy"] = {"decision": "skipped", "by": "gate_dry_run"}
        return out
    order = n * ask
    a = {"purpose": out["purpose"], "order_usd": norm(order), "combined_score": norm(s),
         "instrument": inp["instrument"], "asset_class": inp["asset_class"], "session": inp.get("session", "regular"),
         "unusual_input": False, "first_trade_in_instrument": not inp.get("has_prior_fill", qty > 0),
         "drawdown": inp.get("drawdown", "0"), "daily_pnl_fraction": inp.get("daily_pnl_fraction", "0"),
         "position_usd_after": norm(mv + working + order), "gross_usd_after": norm(gross_now + order),
         "bought_today_usd": norm(D(inp.get("bought_today_usd", "0")) + order),
         "position_pnl_fraction": inp.get("position_pnl_fraction", "0")}
    out["autonomy"] = autonomy(m, a)
    return out

# ------------------------------------------------------------------ research agent: admission (§8.5)
ADMISSION_IGNORED = ("direction_not_allowed", "horizon_mismatch", "revision_without_predecessor")

def stagger_offset(workspace_id, thesis_id, window_s):
    """§8.4 (DEC-100): a deterministic per-workspace delay on the first opening order of a thesis."""
    if window_s <= 0:
        return 0
    digest = hashlib.sha256(f"{workspace_id}\x00{thesis_id}".encode()).digest()
    return int.from_bytes(digest, "big") % window_s

def thesis_expires_at(th):
    return T(th["as_of"]) + timedelta(seconds=th["horizon_s"])

def admission_checks(m, inp):
    """The ordered §8.5 checks as (reason, failed) pairs; the first failure decides."""
    u, beh = m["universe"], m["behavior"]
    th, res = inp["thesis"], beh["research"]
    active = list(inp.get("working_universe", []))
    inst = th["instrument_id"]
    renewal = inst in active
    admitting = [s for s in beh["signal_models"] if s["admits_instruments"]]
    lineage = inp.get("lineages", {}).get(th["lineage_id"], {})
    cap = res["max_revisions_per_lineage"] if res is not None else 0
    spend = D(inp.get("research_spend_usd_today", "0"))
    yield "direction_not_allowed", th["direction"] != "long"
    yield "horizon_mismatch", T(th["expires_at"]) != thesis_expires_at(th)
    yield "revision_without_predecessor", (th["revision"] > 0) != (th.get("predecessor_thesis_id") is not None)
    yield "research_disabled", not admitting or res is None
    yield "universe_pinned", u["pinned"]
    yield "admission_denied", m["autonomy"]["admission"] == "deny"
    yield "cost_cap_reached", res is not None and spend >= D(res["cost_cap_usd_per_day"])
    yield "not_in_data_universe", inp.get("data_universe") is not None and inst not in inp["data_universe"]
    yield "operator_halt", inst in inp.get("halted_instruments", [])
    yield "not_allowed_asset_class", th["asset_class"] not in u["asset_classes"]
    yield "leveraged_etp_not_enabled", bool(th.get("leveraged_etp")) and not (
        u["leveraged_etps_enabled"] and u["leveraged_etp_disclosure_version"] in inp.get("disclosures_accepted", []))
    yield "eligibility_floor", inst in inp.get("eligibility_failures", [])
    yield "instrument_group_claimed", _group_claimed(inst, inp)
    yield "source_not_allowlisted", any(s not in inp.get("allowlisted_sources", []) for s in th["evidence_sources"])
    yield "no_corroboration", not (th["corroboration"] or {}).get("kind")
    yield "lineage_retired", lineage.get("retired", False) or th["revision"] > cap
    yield "universe_full", not renewal and len(active) >= u["max_instruments"]

def _group_claimed(inst, inp):
    groups = inp.get("instrument_groups", {})
    claimed = {groups.get(a, a) for a in inp.get("claimed_by_other_agents", [])}
    return groups.get(inst, inst) in claimed

def admit(m, inp):
    """§8.5: decides one thesis against the envelope. Admission never loosens an envelope field (MI-16)."""
    th = inp["thesis"]
    inst = th["instrument_id"]
    active = list(inp.get("working_universe", []))
    renewal = inst in active
    reason = next((name for name, failed in admission_checks(m, inp) if failed), None)
    kind = "ThesisRevised" if th["revision"] > 0 else "ThesisProposed"
    entry = {"type": kind, "thesis_id": th["thesis_id"], "lineage_id": th["lineage_id"], "revision": th["revision"],
             "instrument": inst, "direction": th["direction"], "horizon_s": th["horizon_s"],
             "conviction": th["conviction"], "confidence": th["confidence"],
             "corroboration": (th["corroboration"] or {}).get("kind"), "admitted": reason is None,
             "reason": reason}
    if th["revision"] > 0:
        entry["predecessor_thesis_id"] = th.get("predecessor_thesis_id")
    out = {"admitted": reason is None, "reason": reason, "ignored": reason in ADMISSION_IGNORED,
           "change": None, "working_universe": active, "journal": [entry]}
    if reason is None:
        out["change"] = "renewed" if renewal else "admitted"
        if not renewal:
            out["working_universe"] = sorted(active + [inst])
            out["journal"].append({"type": "UniverseChanged", "instrument": inst, "change": "admitted",
                                   "reason": "thesis_admitted", "thesis_id": th["thesis_id"],
                                   "universe_size_after": len(out["working_universe"])})
    out["universe_size_after"] = len(out["working_universe"])
    action = inp.get("admission_action")
    out["first_order_autonomy"] = None if reason is not None or action is None else \
        autonomy(m, dict(action, purpose="open", new_instrument=True, thesis_confidence=th["confidence"]))
    return out

def lineage_fold(m, inp):
    """§8.6 (DEC-111): folds a sequence of proposals, capping revisions per lineage.

    Retiring a lineage removes its instrument at once (reason `lineage_retired`): the platform has
    failed on the idea `max_revisions_per_lineage` times and no renewal can be admitted, so leaving
    the position open would leave it with no path back. Removal is exits-only, so it adds no risk.

    Retirement follows the journaled refusal reason, never the revision number alone, so a thesis an
    earlier §8.5 check refused retires nothing. A lineage holds an instrument only until another
    lineage's thesis for it is admitted, so retirement never removes what another lineage holds.
    """
    res = m["behavior"]["research"]
    cap = res["max_revisions_per_lineage"] if res is not None else 0
    lineages, universe, steps = {}, list(inp.get("working_universe", [])), []
    holders = dict(inp.get("lineage_instruments", {}))
    for th in inp["theses"]:
        one = dict(inp, thesis=th, working_universe=universe, lineages=lineages)
        r = admit(m, one)
        universe = r["working_universe"]
        journal = list(r["journal"])
        st = lineages.setdefault(th["lineage_id"], {"revisions": 0, "admitted": 0, "retired": False})
        if r["reason"] == "lineage_retired" and not st["retired"]:
            st["retired"] = True
            held = holders.get(th["lineage_id"])
            if held in universe:
                universe = [i for i in universe if i != held]
                journal.append({"type": "UniverseChanged", "instrument": held, "change": "removed",
                                "reason": "lineage_retired", "thesis_id": th["thesis_id"],
                                "universe_size_after": len(universe)})
        elif r["admitted"]:
            st["revisions"] = max(st["revisions"], th["revision"])
            st["admitted"] += 1
            for lid in [k for k, v in holders.items() if v == th["instrument_id"] and k != th["lineage_id"]]:
                del holders[lid]
            holders[th["lineage_id"]] = th["instrument_id"]
        steps.append({"thesis_id": th["thesis_id"], "admitted": r["admitted"], "reason": r["reason"],
                      "score_carried_forward": False, "lineage_revisions": st["revisions"],
                      "lineage_retired": st["retired"], "universe_size_after": len(universe),
                      "journal": journal})
    return {"steps": steps, "lineages": lineages, "lineage_instruments": holders,
            "working_universe": universe}

def thesis_expiry(m, inp):
    """§8.6, DEC-118: at its horizon a thesis is not renewed; its instrument becomes removed.

    Retirement is read from the lineage state the fold produced (`lineages`), never taken as a
    per-entry flag, so nothing can be removed for a reason the journal does not carry.
    """
    now = T(inp["now"])
    lineages = inp.get("lineages", {})
    removals, keep = [], []
    for e in sorted(inp["entries"], key=lambda x: x["instrument"]):
        if e.get("invalidated"):
            why = "thesis_invalidated"
        elif lineages.get(e["lineage_id"], {}).get("retired", False):
            why = "lineage_retired"
        elif now >= T(e["expires_at"]):
            why = "thesis_expired"
        else:
            keep.append(e["instrument"])
            continue
        removals.append({"type": "UniverseChanged", "instrument": e["instrument"], "change": "removed",
                         "reason": why, "thesis_id": e["thesis_id"], "universe_size_after": None})
    for i, r in enumerate(removals):
        r["universe_size_after"] = len(keep) + len(removals) - i - 1
    return {"working_universe": keep, "removed": [r["instrument"] for r in removals],
            "instrument_restrictions": {r["instrument"]: "removed_instrument" for r in removals},
            "journal": removals}

# ------------------------------------------------------------------ agent-scoped kill switch (trading §5.5)
def agent_flatten(inp):
    agent = inp["agent"]
    owner = inp.get("initiator") == "owner"
    confirmed = inp.get("owner_confirmed_bid", False)
    floor = None
    if owner and confirmed:
        floor = D(inp.get("owner_floor_price") or D(inp["confirmed_bid"]) * (1 - D(inp["max_exit_offset"])))
    cancels = sorted(o["client_order_id"] for o in inp["open_orders"] if o["agent"] == agent)
    sells, deferred = [], []
    for p in inp["agent_positions"]:
        if p["agent"] != agent or D(p["qty"]) == 0:
            continue
        e = {"instrument": p["instrument"], "qty": p["qty"]}
        if p["asset_class"] == "us_equity" and inp["session"] != "regular" and not (owner and confirmed):
            deferred.append(dict(e, until="regular_session_open"))
        elif p["asset_class"] == "us_equity" and inp["session"] != "regular":
            sells.append(dict(e, pricing="exit_price_ladder", floor_price=norm(floor),
                              remainder="rests_at_floor_then_waits_for_open"))
        else:
            sells.append(dict(e, pricing="exit_price_ladder" if inp["session"] != "regular" else "market_or_ladder"))
    return {"mode_applied_first": "stopped" if owner else "paused", "purpose": "owner_exit" if owner else "risk_exit",
            "cancel_client_order_ids": cancels, "cancel_all_endpoint": False, "close_position_endpoint": False,
            "sells": sells, "deferred_sells": deferred}

# ------------------------------------------------------------------ goals (§3.1)
def goal_status(m, st):
    g = m["goal"]
    now_day = risk_day(st["now"])["risk_day"]
    oc = g.get("on_complete")
    if g.get("end_date") is not None and now_day > g["end_date"]:
        if g["type"] == "profit_stop":
            return {"done": True, "reason": "end_date", "then": "discretionary_exit_all_then_retire", "stop_reason": "end_date"}
        return {"done": True, "reason": "end_date", "then": oc, "stop_reason": "goal_complete"}
    if g["type"] == "profit_stop":
        return {"done": False, "note": "profit_stop is confirmed in the risk state (§3.1, §5.6)"}
    if g["type"] == "accumulate":
        inc = D(st["qty_increment"])
        remaining = D(g["target_qty"]) - D(st["position_qty"])
        if remaining < inc or remaining * D(st["ask"]) < D(st["min_order_usd"]):
            return {"done": True, "reason": "target_qty", "then": oc, "stop_reason": "goal_complete"}
        if D(g["max_spend_usd"]) - D(st["goal_spent_usd"]) < D(st["min_order_usd"]):
            return {"done": True, "reason": "max_spend", "then": oc, "stop_reason": "goal_complete"}
        return {"done": False}
    return {"done": False}

# ------------------------------------------------------------------ change classification (§9)
NUM_MAX = ["/capital/allocation_usd", "/capital/max_loss_from_allocation", "/risk/max_position_usd",
           "/risk/max_position_fraction", "/risk/max_gross_exposure_usd", "/risk/max_order_usd",
           "/risk/max_orders_per_day", "/risk/max_daily_loss", "/risk/max_drawdown", "/risk/breach_confirm_s",
           "/goal/target_qty", "/goal/max_spend_usd", "/goal/max_avg_price", "/goal/profit_level",
           "/protection/stop_distance", "/behavior/sizing/exit_threshold", "/universe/max_instruments",
           "/behavior/research/cost_cap_usd_per_day", "/behavior/research/max_revisions_per_lineage"]
NUM_MIN = ["/behavior/sizing/entry_threshold", "/behavior/sizing/rebalance_band", "/risk/hysteresis",
           "/risk/reentry_cooldown_s", "/risk/daily_breach_min_s", "/risk/scale_lift_after_s",
           "/behavior/research/interval_s"]
NEUTRAL = ["/name", "/notifications/quiet_hours"]

def rule_widen(a, b):
    wa, wb = a["when"], b["when"]
    if not ("field" in wa and "field" in wb and wa["field"] == wb["field"] and wa["op"] == wb["op"]):
        return None
    op, old, new = wa["op"], wa["value"], wb["value"]
    if op in ("lt", "lte"):
        return 1 if D(new) > D(old) else -1
    if op in ("gt", "gte"):
        return 1 if D(new) < D(old) else -1
    if op in ("in", "not_in") and isinstance(old, list):
        so, sn = set(old), set(new)
        more = sn > so if op == "in" else sn < so
        less = sn < so if op == "in" else sn > so
        return 1 if more else (-1 if less else None)
    return None

DELEGATION_CAPS = ("max_order_usd", "max_orders", "max_total_usd")

def delegation_narrowed(a, b):
    """§9.2: same id, lifts, when, and source; no cap larger; a window no wider."""
    same = all(a[k] == b[k] for k in ("id", "lifts", "when", "source_approval_id"))
    return (same and all(D(b[k]) <= D(a[k]) for k in DELEGATION_CAPS)
            and T(b["starts_at"]) >= T(a["starts_at"]) and T(b["expires_at"]) <= T(a["expires_at"]))

def classify_delegations(od, nd):
    """§9.2 `autonomy.delegations`: reducing only if every change removes or narrows a delegation."""
    oi, ni = {d["id"]: d for d in od}, {d["id"]: d for d in nd}
    if [x for x in oi if x in ni] != [x for x in ni if x in oi]:
        return "increasing"
    for i, d in ni.items():
        if i not in oi:
            return "increasing"
        if d != oi[i] and not delegation_narrowed(oi[i], d):
            return "increasing"
    return "reducing"

def classify_autonomy(o, n, lifted=frozenset()):
    """§9.2's autonomy row. `lifted` is the asks the new version's delegations name: a change that sends an order
    which used to reach an undelegated ask to one of them would let a delegation decide it less strictly, so it is
    increasing (MI-29, DEC-353). An order that was `auto` and lands on one stays `auto`, so removing or narrowing an
    `auto` rule stays reducing."""
    o, n = ({k: v for k, v in x.items() if k != "tripwires"} for x in (o, n))
    od, nd = o.get("delegations", []), n.get("delegations", [])
    if od or nd:
        o, n = {k: v for k, v in o.items() if k != "delegations"}, {k: v for k, v in n.items() if k != "delegations"}
        rest = classify_autonomy(o, n, frozenset(d["lifts"] for d in nd)) if o != n else None
        parts = ([classify_delegations(od, nd)] if od != nd else []) + ([rest] if rest else [])
        return "increasing" if "increasing" in parts else "reducing"
    oa, na = o["approval"], n["approval"]
    if oa["approvers"] != na["approvers"] or oa["timeout_s"] != na["timeout_s"] or oa["on_timeout"] != na["on_timeout"]:
        return "increasing"
    ot, nt = oa["two_approver_above_usd"], na["two_approver_above_usd"]
    if ot != nt and (nt is None or (ot is not None and D(nt) > D(ot))):
        return "increasing"
    if STRICT[n["default"]] < STRICT[o["default"]] or STRICT[n["admission"]] < STRICT[o["admission"]]:
        return "increasing"
    oids, nids = [r["id"] for r in o["rules"]], [r["id"] for r in n["rules"]]
    common_o = [x for x in oids if x in nids]
    common_n = [x for x in nids if x in oids]
    if common_o != common_n:
        return "increasing"
    for i, ra in enumerate(o["rules"]):
        if ra["id"] not in nids:
            later = [STRICT[x["then"]] for x in o["rules"][i + 1:]] + [STRICT[o["default"]]]
            if any(x < STRICT[ra["then"]] for x in later):
                return "increasing"
            later_sources = {f"rule:{x['id']}" for x in o["rules"][i + 1:]} | {"default"}
            if ra["then"] != "auto" and later_sources & lifted:
                return "increasing"
    orules = {r["id"]: r for r in o["rules"]}
    for i, rb in enumerate(n["rules"]):
        later = [STRICT[x["then"]] for x in n["rules"][i + 1:]] + [STRICT[n["default"]]]
        ra = orules.get(rb["id"])
        if ra is None:
            if any(x > STRICT[rb["then"]] for x in later):
                return "increasing"
            continue
        if ra == rb:
            continue
        if ra["when"] == rb["when"]:
            if STRICT[rb["then"]] < STRICT[ra["then"]]:
                return "increasing"
            continue
        if ra["then"] != rb["then"]:
            return "increasing"
        d = rule_widen(ra, rb)
        if d is None:
            return "increasing"
        t = STRICT[ra["then"]]
        if t == 0 and d > 0:
            return "increasing"
        if t > 0 and (d < 0 or any(x > t for x in later)):
            return "increasing"
        if rb["then"] == "ask" and d > 0 and f"rule:{rb['id']}" in lifted:
            return "increasing"
    return "reducing"

def diff_paths(a, b, base=""):
    if isinstance(a, dict) and isinstance(b, dict):
        for k in sorted(set(a) | set(b)):
            yield from diff_paths(a.get(k), b.get(k), f"{base}/{k}")
    elif a != b:
        yield base

PIN_SWITCH_PATHS = ["/universe/pinned", "/universe/pinned_instruments", "/universe/max_instruments",
                    "/behavior/research", "/behavior/signal_models"]

def pinning_switch(old, new, paths):
    """§9.2, DEC-121: turning bring-your-own-strategy on is one risk-reducing change, not a field-by-field one."""
    ou, nu, ob, nb = old["universe"], new["universe"], old["behavior"], new["behavior"]
    if ou["pinned"] or not nu["pinned"] or nb["research"] is not None:
        return False
    if not any(s["admits_instruments"] for s in ob["signal_models"]):
        return False
    if nu["max_instruments"] > ou["max_instruments"]:
        return False
    if nb["signal_models"] != [dict(s, admits_instruments=False) for s in ob["signal_models"]]:
        return False
    return all(covered(p, PIN_SWITCH_PATHS) for p in paths)

def classify_tripwires(ot, nt):
    """§9.2 `autonomy.tripwires` (DEC-352): matched by id. Reducing only if every change adds a tripwire, lowers a
    threshold, or makes an action stricter; removing one, raising a threshold, softening an action, or changing a
    metric is increasing. An empty list and no list are the same."""
    if ot == nt:
        return "neutral"
    ni = {t["id"]: t for t in nt}
    for a in ot:
        b = ni.get(a["id"])
        if (b is None or b["metric"] != a["metric"] or D(b["threshold"]) > D(a["threshold"])
                or TRIPWIRE_ACTIONS[b["action"]] < TRIPWIRE_ACTIONS[a["action"]]):
            return "increasing"
    return "reducing"

def classify(old, new):
    res = set()
    if old["mandate_schema_version"] != new["mandate_schema_version"]:
        old, new = upcast(old), upcast(new)
    paths = list(diff_paths(old, new))
    if "/environment" not in paths and "/connection_id" not in paths and pinning_switch(old, new, paths):
        return "risk_reducing", paths
    for p in paths:
        a, b = get_or_none(old, p), get_or_none(new, p)
        if p in ("/environment", "/connection_id"):
            return "invalid", paths
        if any(p == x or p.startswith(x + "/") for x in NEUTRAL):
            res.add("neutral")
        elif p == "/notifications/channels":
            res.add("increasing" if set(a) - set(b) else "neutral")
        elif p in NUM_MAX:
            res.add("increasing" if (b is None or (a is not None and D(b) > D(a))) else "reducing")
        elif p in NUM_MIN:
            res.add("increasing" if D(b) < D(a) else "reducing")
        elif p == "/risk/scale_action":
            res.add("reducing" if b == "trim_to_target" else "increasing")
        elif p == "/risk/drawdown_ladder":
            if [x["action"] for x in a] != [x["action"] for x in b]:
                res.add("increasing")
            else:
                for x, y in zip(a, b):
                    if D(y["at"]) > D(x["at"]) or (x["factor"] is not None and D(y["factor"]) > D(x["factor"])):
                        res.add("increasing")
                    elif x != y:
                        res.add("reducing")
        elif p == "/universe/pinned_instruments":
            if old["universe"]["pinned"] != new["universe"]["pinned"]:
                continue
            oi, ni = {i["asset_id"] for i in a}, {i["asset_id"] for i in b}
            res.add("increasing" if ni - oi else ("reducing" if oi - ni else "neutral"))
        elif p == "/universe/pinned":
            had_agent = any(s["admits_instruments"] for s in old["behavior"]["signal_models"])
            res.add("reducing" if b and had_agent else "increasing")
        elif p == "/universe/asset_classes":
            res.add("increasing" if set(b) - set(a) else "reducing")
        elif p == "/behavior/research":
            res.add("increasing" if b is not None else "reducing")
        elif p == "/autonomy/review_by":
            res.add("increasing" if b is None or (a is not None and b > a) else "reducing")
        elif p == "/autonomy/tripwires":
            res.add(classify_tripwires(a or [], b or []))
        elif p.startswith("/autonomy"):
            res.add(classify_autonomy(old["autonomy"], new["autonomy"]))
        elif p == "/goal/end_date":
            res.add("increasing" if b is None or (a is not None and b > a) else "reducing")
        elif p in ("/protection/crypto_stop_limit_offset", "/protection/stop_limit_offset"):
            res.add("increasing" if b is not None and (a is None or D(b) > D(a)) else "reducing")
        elif p == "/protection/enabled":
            res.add("increasing" if not b else "reducing")
        elif p == "/universe/leveraged_etps_enabled":
            res.add("increasing" if b else "reducing")
        else:
            res.add("increasing")
    if "increasing" in res:
        return "risk_increasing", paths
    if "reducing" in res:
        return "risk_reducing", paths
    return "neutral", paths

# ------------------------------------------------------------------ escalation (§6.1, §6.4; DEC-155, DEC-156, DEC-158, DEC-173)
STEP_UP_WINDOW_S = 300
ASK_BUDGET_PER_RISK_DAY = 10
DRIFT_BAND_BP = {"us_equity": 100, "crypto": 200}
BOUND_FIELDS = ("instrument", "side", "qty", "limit_price", "purpose", "mandate_version")
CANCEL_REASONS = ("version_applied", "mode_tightened", "owner_pause", "owner_stop", "kill_switch")
CONTENT_KEYS = ("action", "trigger", "evidence", "risk_impact", "reference_mark", "deadline", "default", "choices", "approvers")
DEFAULT_SENTENCE = "If you do nothing, this action is skipped"
SCORE_LABEL = "combined model score, not a probability of profit"

def approval_trigger(m, req):
    """The §6.4 content object's `trigger`: the owner's confirmed rule verbatim when a rule asked, else null."""
    rules = {x["id"]: x for x in m["autonomy"]["rules"]}
    by = req["decided_by"]
    rule = rules.get(by[len("rule:"):]) if by.startswith("rule:") else None
    return {"mandate_version": req["mandate_version"], "decided_by": by, "requested_by": req.get("requested_by", "agent"),
            "client": req.get("client") if req.get("requested_by") == "client" else None,
            "rule": None if rule is None else {"id": rule["id"], "when": rule["when"], "then": rule["then"]}}

def approval_content(m, req, figures):
    """The §6.4 content object whose SHA-256 is the content hash; `figures` are the §6.3 values at the request."""
    r = m["risk"]
    E = D(figures["agent_equity"])
    ladder = [D(x["at"]) for x in r["drawdown_ladder"]]
    caps = {"order_usd": r["max_order_usd"], "position_usd_after": norm(min(D(r["max_position_usd"]), D(r["max_position_fraction"]) * E)),
            "gross_usd_after": norm(min(D(r["max_gross_exposure_usd"]), E)), "bought_today_usd": None,
            "drawdown": norm(min(ladder)) if ladder else None, "daily_pnl_fraction": r["max_daily_loss"]}
    return {
        "action": {k: req[k] for k in ("instrument", "asset_class", "side", "qty")} | {"limit": req["limit_price"]}
                  | {"order_usd": norm(D(req["qty"]) * D(req["limit_price"])), "purpose": req["purpose"]},
        "trigger": approval_trigger(m, req),
        "evidence": {"combined_score": {"value": req["combined_score"], "label": SCORE_LABEL}, "outputs": req.get("outputs", [])},
        "risk_impact": [{"field": f, "value": norm(D(req["qty"]) * D(req["limit_price"])) if f == "order_usd" else figures[f], "cap": caps[f]}
                        for f in ("order_usd", "position_usd_after", "gross_usd_after", "bought_today_usd", "drawdown", "daily_pnl_fraction")],
        "reference_mark": req["reference_mark"],
        "deadline": req["deadline"],
        "default": DEFAULT_SENTENCE,
        "choices": ["approve", "skip"] + list(req.get("delegation_shapes", [])),
        "approvers": {"required": req["approvers_required"], "independent": req["independent_required"]},
    }

def content_hash(content):
    return "sha256:" + hashlib.sha256(canon(content).encode()).hexdigest()

def approval_notification(req):
    """Rule 6: the request's opaque approval id and one generic text; nothing else of the request leaves."""
    return {"subject": req["approval"], "text": "approval_needed"}

def step_up_fault(ev, at, environment, used):
    """§6.4 step-up judged at `at`: the first of missing, stale, reused, method, or None when the evidence counts.
    Fresh is 0 to 300 s old inclusive; evidence authenticated after `at` is stale, so clock skew fails closed."""
    if ev is None:
        return "step_up_missing"
    age = (T(at) - T(ev["authenticated_at"])).total_seconds()
    if not 0 <= age <= STEP_UP_WINDOW_S:
        return "step_up_stale"
    if ev["assertion"] in used:
        return "step_up_reused"
    if not (ev["method"] == "cli_confirm" and environment == "paper"):
        return "step_up_method"
    return None

def evidence_fault(ev, at, environment, used):
    """Malformed evidence counts as missing."""
    try:
        return step_up_fault(ev, at, environment, used)
    except (KeyError, TypeError, ValueError, AssertionError):
        return "step_up_missing"

NO_POLICY = {"independent_approval_required": False, "two_approver_above_usd": None}

def approval_quorum(req, policy):
    """§6.4 check 7's requirement: the stricter of what the request bound and the workspace policy overlay (§4.3)
    current at the response. Independence if either requires it; the larger approver count, where a policy ceiling
    below the order value asks for two. A policy change can only tighten a pending approval (DEC-173 item 13)."""
    ceiling = policy["two_approver_above_usd"]
    by_policy = 2 if ceiling is not None and D(req["qty"]) * D(req["limit_price"]) > D(ceiling) else 1
    return {"required": max(req["approvers_required"], by_policy),
            "independent": req["independent_required"] or policy["independent_approval_required"]}

def approval_admit(req, resp, ctx):
    """§6.4 admission, checks 1 to 7 in order. `req` is the request as the fold holds it after this step's own
    cancellations, or None when it is not pending. A skip runs checks 1 to 5 only."""
    eff = max(T(resp["submitted_at"]), T(ctx["clock"]))
    out = lambda result, reason=None: {"result": result, "reason": reason, "effective_at": fmt(eff)}
    if req is None or req["approval"] != resp["approval"]:
        return out("refused", "not_pending")
    if eff >= T(req["deadline"]):
        return out("refused", "late")
    if resp["actor_kind"] != "user" or resp["responder"] not in ctx["approvers"]:
        return out("refused", "not_an_approver")
    if not req["delivered"]:
        return out("refused", "not_delivered")
    if resp["content_hash"] != req["content_hash"]:
        return out("refused", "content_mismatch")
    if resp["verdict"] == "skipped":
        return out("admitted")
    fault = evidence_fault(resp["step_up"], fmt(eff), ctx["environment"], ctx["used_assertions"])
    if fault:
        return out("refused", fault)
    q = approval_quorum(req, ctx["policy"])
    judged = lambda result, reason=None: out(result, reason) | {"quorum": q}
    if resp["responder"] in req["grants"]:
        return judged("refused", "duplicate_approver")
    if q["independent"] and resp["responder"] == ctx["author"]:
        return judged("refused", "not_independent")
    counting = {g for g in req["grants"] if not (q["independent"] and g == ctx["author"])}
    return judged("admitted" if len(counting) + 1 >= q["required"] else "counted")

def within_drift(m_req, m_now, asset_class):
    """§6.4 drift: |m_now − m_req| × 10 000 ≤ band_bp × m_req, exact, no division; no mark at either end is outside."""
    if m_req is None or m_now is None:
        return False
    return abs(D(m_now) - D(m_req)) * 10000 <= DRIFT_BAND_BP[asset_class] * D(m_req)


def approval_intent(req):
    return {
        "instrument_id": req["instrument"],
        "side": req["side"],
        "order_type": "limit",
        "tif": "day" if req["asset_class"] == "us_equity" else "gtc",
        "qty": req["qty"],
        "limit_price": req["limit_price"],
        "purpose": req["purpose"],
        "mandate_version": req["mandate_version"],
    }


def approval_revalidate(req, now):
    """§6.4 re-validation, checks 8 to 12 in order, for a grant admitted in the same step. It only skips: the act
    carries the bound fields unchanged. Check 9's `mode` arm is reached by no step the spec names: a step whose
    mode is exits-only or stricter cancels every pending approval before it judges a response (DEC-318, DEC-430)."""
    skip = lambda reason: {"result": "skip", "reason": reason}
    if now["mandate_version"] != req["mandate_version"]:
        return skip("version_changed")
    if now["mode"] != "normal":
        return skip("mode")
    if now["instrument_restricted"] or not now["in_working_universe"]:
        return skip("instrument_restricted")
    c = now["classification"]
    if c["decision"] == "deny":
        return skip("reclassified_deny")
    if c["decision"] == "ask" and c["by"] != req["decided_by"]:
        return skip("reclassified_other_trigger")
    if now["dry_run"]["verdict"] != "allow":
        return skip(now["dry_run"]["reason"])
    if not within_drift((req["reference_mark"] or {}).get("price"), now["mark"], req["asset_class"]):
        return skip("drift")
    intent = approval_intent(req)
    return {
        "result": "act",
        "reason": None,
        "intent": intent,
    }

def ask_permit(ledger, instrument, at):
    """§6.4 anti-fatigue bounds over one agent's journaled asks, in the precedence budget, skipped_today,
    recent_timeout. A suppressed ask is not an ApprovalRequested, so it is not in the ledger and does not count."""
    today = risk_day(at)["risk_day"]
    asked, skipped, timed_out = 0, False, False
    for e in ledger:
        if e["event"] == "requested" and risk_day(e["at"])["risk_day"] == today:
            asked += 1
        elif e["event"] == "owner_skipped" and e["instrument"] == instrument and risk_day(e["at"])["risk_day"] == today:
            skipped = True
        elif e["event"] == "version_applied":
            skipped = False
        elif e["event"] == "timed_out" and e["instrument"] == instrument and 0 <= (T(at) - T(e["at"])).total_seconds() < e["timeout_s"]:
            timed_out = True
    if asked >= ASK_BUDGET_PER_RISK_DAY:
        return "budget"
    if skipped:
        return "skipped_today"
    if timed_out:
        return "recent_timeout"
    return None

def deliver_now(channel, quiet_hours, at):
    """§6.4 quiet hours: a push inside [start, end) America/New_York wall time is suppressed; the inbox always delivers."""
    if channel == "cli_inbox" or quiet_hours is None:
        return "delivered"
    local = T(at).astimezone(NY)
    minute = local.hour * 60 + local.minute
    start, end = (int(x[:2]) * 60 + int(x[3:5]) for x in (quiet_hours["start"], quiet_hours["end"]))
    return "suppressed_quiet_hours" if (minute - start) % 1440 < (end - start) % 1440 else "delivered"

def owner_command(kind, ev, committed_at, processed_at, environment, used):
    """§6.1 owner controls: pause always applies; resume, stop, and acknowledge are judged when the runtime processes
    them; an owner exit when the owner committed it. The kill switch is `owner_kill_switch`, which never refuses."""
    if kind == "pause":
        return {"result": "apply", "reason": None}
    at = committed_at if kind == "owner_exit" else processed_at
    fault = evidence_fault(ev, at, environment, used)
    return {"result": "apply" if fault is None else "refused", "reason": fault}

def owner_exit_order(m, st, prop, mode, session, asset_class, confirmed_bid, authority):
    """§6.1: a refused owner exit loses only the owner-exit privilege (an equity sale outside the regular session at the
    confirmed bid); the exit itself is still routed, as a regular-session exit, and never dropped."""
    privilege = confirmed_bid and authority["result"] == "apply"
    return order_decision(m, st, prop, mode, set(), session, False, owner_confirmed_bid=privilege, asset_class=asset_class)

def owner_kill_switch(inp, ev, committed_at, environment, used):
    """§6.1 and DEC-158 option (c): never refused. Without valid evidence as committed it still stops the agent and
    flattens as an automated flatten does (equities wait for the regular session); with it, the owner-exit privilege applies."""
    fault = evidence_fault(ev, committed_at, environment, used)
    out = agent_flatten(dict(inp, initiator="owner", owner_confirmed_bid=inp.get("owner_confirmed_bid", False) and fault is None))
    return dict(out, step_up=fault)

def proposal_route(pending, purpose):
    """Rule 13: nothing waits on an approval but a risk-adding proposal, which waits while one is pending (§6.4)."""
    if purpose in REDUCING:
        return "handed"
    return "awaiting_approval" if pending else "evaluate"

def escalation_apply(st, e):
    """The approval fold over the runtime's own journal and the scheduler's `ClockAdvanced`, which the runtime folds
    before it steps a tick, so replay and restart give the same answers."""
    st["clock"] = max(st["clock"], T(e["clock"]))
    a, t = e.get("approval"), e["type"]
    pending = st["pending"]
    if t == "ApprovalRequested":
        pending[a] = {k: e[k] for k in e if k not in ("type", "clock")} | {"delivered": False, "grants": set()}
    elif t == "ApprovalDelivered" and e["status"] == "delivered" and a in pending:
        pending[a]["delivered"] = True
    elif t == "ApprovalResponded":
        st["copied"].add(e["source"])
        if e["step_up"] is not None and isinstance(e["step_up"], dict) and "assertion" in e["step_up"]:
            st["used"].add(e["step_up"]["assertion"])
        if a in pending and e["verdict"] == "approved" and e["result"] in ("admitted", "counted"):
            pending[a]["grants"].add(e["responder"])
        if a in pending and e["verdict"] == "skipped" and e["result"] == "admitted":
            del pending[a]
    elif t in ("ApprovalRevalidated", "ApprovalTimedOut", "ApprovalCanceled"):
        pending.pop(a, None)
    elif t == "PolicyChanged":
        st["policy"] = {k: e[k] for k in NO_POLICY}
    return st

def escalation_fold(journal, t0):
    st = {"clock": T(t0), "pending": {}, "copied": set(), "used": set(), "policy": dict(NO_POLICY)}
    for e in journal:
        escalation_apply(st, e)
    return st

def escalation_step(st, inp, ctx):
    """One runtime step over the approval lifecycle (§6.4). Returns the drafts, in order; the caller folds them.
    A response read in a step whose mode is exits-only or stricter first cancels every pending approval as
    `mode_tightened` (§6.4 "Cancellation", DEC-318 option (a)); the runtime's `AgentModeChanged` before it is the
    mode's record, which the model does not draft.
    Every draft carries the step's risk clock. The reference hashes the canonical request as a stand-in for the
    content object, which `approval_content` builds and `fuzz_content` checks on its own."""
    kind = inp["kind"]
    clock = max(st["clock"], T(inp["at"])) if kind == "tick" else st["clock"]
    c = fmt(clock)
    drafts = []
    if kind == "ask":
        if proposal_route(st["pending"], inp["bound"]["purpose"]) != "evaluate":
            return drafts
        req = dict(inp["bound"], approval=inp["approval"], deadline=fmt(clock + timedelta(seconds=ctx["timeout_s"])))
        req["content_hash"] = content_hash({k: req[k] for k in sorted(req)})
        drafts.append(dict(req, type="ApprovalRequested", clock=c))
        for channel in ["cli_inbox"] * ctx["inbox"] + ctx["push_channels"]:
            drafts.append({"type": "ApprovalDelivered", "approval": inp["approval"], "channel": channel,
                           "status": deliver_now("cli_inbox" if channel == "cli_inbox" else "push", ctx["quiet_hours"], c), "clock": c})
    elif kind == "tick":
        for a in sorted(st["pending"]):
            if T(st["pending"][a]["deadline"]) <= clock:
                drafts.append({"type": "ApprovalTimedOut", "approval": a, "on_timeout": "skip", "clock": c})
    elif kind in ("cancel", "batch"):
        for a in sorted(st["pending"]):
            drafts.append({"type": "ApprovalCanceled", "approval": a, "reason": inp["reason"], "clock": c})
        after = copy.deepcopy(st)
        for d in drafts:
            escalation_apply(after, d)
        for resp in inp.get("responses", []):
            out = escalation_step(after, {"kind": "response", "response": resp, "now": inp["now"]}, ctx)
            for d in out:
                escalation_apply(after, d)
            drafts += out
    elif kind == "response":
        resp = inp["response"]
        if inp["now"]["mode"] != "normal" and st["pending"]:
            for a in sorted(st["pending"]):
                drafts.append({"type": "ApprovalCanceled", "approval": a, "reason": "mode_tightened", "clock": c})
            st = copy.deepcopy(st)
            for d in drafts:
                escalation_apply(st, d)
        if resp["source"] in st["copied"]:
            return drafts
        req = st["pending"].get(resp["approval"])
        adm = approval_admit(req, resp, dict(ctx, clock=c, used_assertions=st["used"], policy=st["policy"]))
        drafts.append({"type": "ApprovalResponded", "approval": resp["approval"], "source": resp["source"],
                       "responder": resp["responder"], "verdict": resp["verdict"], "step_up": resp["step_up"],
                       "clock": c} | adm)
        if adm["result"] == "admitted" and resp["verdict"] == "approved":
            rv = approval_revalidate(req, inp["now"])
            drafts.append({"type": "ApprovalRevalidated", "approval": resp["approval"], "result": rv["result"],
                           "reason": rv["reason"], "clock": c})
            if rv["result"] == "act":
                drafts.append(dict(rv["intent"], type="IntentProposed", approval=resp["approval"], clock=c))
    return drafts
