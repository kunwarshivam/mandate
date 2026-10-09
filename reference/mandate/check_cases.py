"""Asserts that every reference case demonstrates what its title claims (AGENTS.md: validate fixtures)."""
import hashlib
import json
import pathlib
import re
import sys
from decimal import ROUND_CEILING, ROUND_DOWN, Decimal
from datetime import datetime, time, timezone
from zoneinfo import ZoneInfo
import yaml
d = yaml.safe_load(open(pathlib.Path(__file__).resolve().parents[2] / "docs/specs/reference-cases/mandate.yaml"))
C = {c["id"]: c for c in d["cases"]}
bad = []

def req(cid, ok, why):
    if not ok:
        bad.append((cid, why))

def steps(cid):
    return [s["expect"] for s in C[cid]["steps"]]

def journ(x):
    return [(j["type"], j.get("limit"), j.get("reason")) for j in x["journal"]]

for c in d["cases"]:
    cid, k, e = c["id"], c["kind"], c.get("expect")
    if k == "schema":
        req(cid, e["schema_valid"] == (cid in ("MC-S01", "MC-K01")), "only S01 and K01 (version 2) are valid")
    if k == "semantic":
        code = {"V-01": None}
        t = c["title"].lower()
        passes = any(w in t for w in ("passes", "exactly equal", "with the accepted", "is fine", "platform defaults on listed",
                                      "user-entered and confirmed", "with two approvers", "equal to the validation",
                                      "below the floor budget", "is a warning", "shadowed", "(warning w-003)",
                                      "review date at the platform default is valid", "review date on the validation date is valid",
                                      "180 days after validation is valid", "lapsed review date carried unchanged stays valid",
                                      "moves the review date and carries the delegation over"))
        req(cid, (e["violations"] == []) == passes, f"violations {e['violations']} vs title")
    if k == "policy":
        t = c["title"].lower()
        req(cid, e["valid"] == ("equal to" in t or "conforms" in t), "validity vs title")
        if "nearest" in t:
            req(cid, e["violations"][0]["limit_level"] == "workspace", "nearest level")

# policy specifics
req("MC-P05", {v["key"] for v in C["MC-P05"]["expect"]["violations"]} == {"research_agent_allowed"}, "retail refuses the research agent")
req("MC-P14", {v["key"] for v in C["MC-P14"]["expect"]["violations"]} == set(), "retail now allows auto and llm (DEC-98)")
req("MC-P04", C["MC-P04"]["expect"]["violations"][0]["level"] == "workspace", "workspace itself invalid")
# semantic specifics
exp = {"MC-V03": "V-002", "MC-V05": "V-003", "MC-V06": "V-005", "MC-V08": "V-006", "MC-V09": "V-007", "MC-V10": "V-007",
       "MC-V11": "V-008", "MC-V12": "V-008", "MC-V14": "V-009", "MC-V15": "V-009", "MC-V16": "V-010", "MC-V17": "V-010",
       "MC-V18": "V-011", "MC-V19": "V-012", "MC-V20": "V-013", "MC-V21": "V-014", "MC-V22": "V-015", "MC-V23": "V-016",
       "MC-V24": "V-017", "MC-V25": "V-018", "MC-V26": "V-020", "MC-V27": "V-020", "MC-V29": "V-020", "MC-V30": "V-020",
       "MC-V31": "V-022", "MC-V33": "V-023", "MC-V34": "V-023", "MC-V35": "V-023", "MC-V36": "V-023", "MC-V37": "V-023",
       "MC-V38": "V-023", "MC-V39": "V-024", "MC-V41": "V-030", "MC-V43": "V-031", "MC-V44": "V-032",
       "MC-V49": "V-020", "MC-V50": "V-020", "MC-V51": "V-033",
       "MC-V53": "V-034", "MC-V54": "V-034", "MC-V55": "V-035", "MC-V56": "V-036", "MC-V57": "V-036",
       "MC-V58": "V-036", "MC-V59": "V-037", "MC-V61": "V-039", "MC-V62": "V-038", "MC-V63": "V-020",
       "MC-V64": "V-022", "MC-V67": "V-020", "MC-V69": "V-047"}
for cid, code in exp.items():
    req(cid, C[cid]["expect"]["violations"] == [code], f"expected exactly {code}")
req("MC-V13", "W-003" in C["MC-V13"]["expect"]["warnings"], "W-003")
req("MC-V46", "W-001" in C["MC-V46"]["expect"]["warnings"], "W-001")
req("MC-V47", "W-005" in C["MC-V47"]["expect"]["warnings"], "W-005")
req("MC-V48", len(C["MC-V48"]["expect"]["violations"]) >= 3, "multiple")
req("MC-V60", C["MC-V60"]["expect"]["violations"] == ["V-003", "V-036"], "accumulate admits nothing, two ways")
req("MC-V65", "W-006" in C["MC-V65"]["expect"]["warnings"], "W-006")
for cid, required, users in (("MC-V69", True, 1), ("MC-V70", True, 2), ("MC-V71", False, 1)):
    ctx = C[cid]["context"]
    req(cid, C[cid]["patch"] == [] and ctx == {"independent_approval_required": required, "workspace_users": users},
        "V-047's trio differs only in the policy and the workspace's users (DEC-411)")
for cid in ("MC-V70", "MC-V71"):
    req(cid, C[cid]["expect"]["violations"] == [], "a second user, or no independence, passes V-047")
lone = {"independent_approval_required": True, "workspace_users": 1}
current = d["bases"]["btc_accumulator"]["mandate"]
current_version = d["bases"]["btc_accumulator"]["canonical_sha256"]
fewer_orders = [{"op": "replace", "path": "/risk/max_orders_per_day", "value": 40}]
for cid, patch, previous, refused in (
        ("MC-V72", fewer_orders, "current", False),
        ("MC-V73", [{"op": "replace", "path": "/name", "value": "btc-accumulator-renamed"}], "current", True),
        ("MC-V74", fewer_orders + [{"op": "replace", "path": "/risk/max_order_usd", "value": "1500"}], "current", True),
        ("MC-V75", fewer_orders, "identity", True),
        ("MC-V76", [{"op": "replace", "path": "/risk/max_order_usd", "value": "5000"}], "forged", True),
        ("MC-V77", fewer_orders, "over_schema", True)):
    ctx = C[cid]["context"]
    prev = ctx["previous_version"]
    shape = {"current": lambda: prev == current,
             "identity": lambda: set(prev) == {"environment", "connection_id"},
             "forged": lambda: prev != current
             and dict(prev, risk=dict(prev["risk"], max_order_usd=None))
             == dict(current, risk=dict(current["risk"], max_order_usd=None))
             and Decimal(prev["risk"]["max_order_usd"]) > Decimal(patch[0]["value"])
             > Decimal(current["risk"]["max_order_usd"]),
             "over_schema": lambda: prev == dict(current, risk=dict(current["risk"], max_orders_per_day=10001))}[previous]()
    own_hash = "sha256:" + hashlib.sha256(json.dumps(prev, sort_keys=True, separators=(",", ":"),
                                                     ensure_ascii=False).encode()).hexdigest()
    hash_named = own_hash if previous in ("identity", "over_schema") else current_version
    req(cid, C[cid]["base"] == "btc_accumulator" and C[cid]["patch"] == patch and shape
        and {k: v for k, v in ctx.items() if k != "previous_version"} == dict(lone, current_mandate_version=hash_named),
        "V-047's exception cases differ from the base only in the patch named and the previous version (DEC-444)")
    req(cid, C[cid]["expect"]["violations"] == (["V-047"] if refused else []),
        "only a risk-reducing version against the agent's current document passes V-047 in a lone workspace (DEC-444)")

# risk state
s = steps("MC-R01")
req("MC-R01", s[1]["size_factor"] == "0.5" and s[4]["size_factor"] == "0.5" and s[5]["size_factor"] == "1", "scale then hysteresis lift at 103 not 102.9")
req("MC-R01", s[6]["agent_mode"] == "exits_only" and s[7]["agent_mode"] == "paused", "exits_only then flatten")
s = steps("MC-R02")
req("MC-R02", s[3]["pending"] == ["drawdown_ladder[1]"] and s[5]["agent_mode"] == "exits_only" and s[4]["agent_mode"] == "normal", "short recovery keeps accumulating")
s = steps("MC-R03")
req("MC-R03", s[4]["agent_mode"] == "normal" and s[5]["agent_mode"] == "exits_only", "long recovery restarts: trigger 60 s after 14:03:40")
s = steps("MC-R04")
req("MC-R04", s[1]["agent_mode"] == "normal" and s[2]["restrictions"] == ["hard_breach"] and
    ("RiskLimitTriggered", "drawdown_ladder[1]", "hard_trigger") in journ(s[3]), "hard trigger after a second quote")
s = steps("MC-R05")
req("MC-R05", s[2]["agent_mode"] == "normal" and s[3]["agent_mode"] == "exits_only", "confirmed on clock after close")
s = steps("MC-R06")
req("MC-R06", s[2]["agent_mode"] == "normal" and ("RiskLimitTriggered", "max_daily_loss", "resolved_at_rollover") in journ(s[3])
    and s[4]["agent_mode"] == "exits_only", "keeps confirming after the rollover, then latches")
s = steps("MC-R07")
req("MC-R07", s[3]["agent_mode"] == "exits_only" and s[4]["agent_mode"] == "normal", "lift waits 3600 s")
s = steps("MC-R08")
req("MC-R08", ("RiskLimitTriggered", "max_daily_loss", "new_day_breach") in journ(s[3]) and s[4]["agent_mode"] == "exits_only", "renewed")
s = steps("MC-R09")
req("MC-R09", s[2].get("error") == "flatten_in_progress" and s[4]["high_water_mark"] == s[4]["agent_equity"]
    and s[4]["size_factor"] == "0.375", "reject then reset with all scale rungs active")
req("MC-R09", s[5]["size_factor"] == "0.375" and s[6]["size_factor"] == "0.75" and s[7]["size_factor"] == "0.75" and s[8]["size_factor"] == "1",
    "higher rung lifts at reset + 600 s (14:20), lower 600 s later (14:30)")
s = steps("MC-R10")
req("MC-R10", "lifetime_floor" in s[1]["restrictions"] and s[2].get("error") == "not_acknowledgeable", "floor with carry; not ackable")
s = steps("MC-R11")
req("MC-R11", s[1]["restrictions"] == [] and s[1]["drawdown"] == s[0]["drawdown"] and s[2]["drawdown"] == s[0]["drawdown"], "no trip, DD preserved")
s = steps("MC-R12")
req("MC-R12", s[2].get("error") == "increase_blocked_while_latched" and s[3].get("error") == "equity_below_exposure", "rejections")
s = steps("MC-R24")
req("MC-R24", s[1]["instrument_restrictions"] == ["removed_instrument"] and s[3]["instrument_restrictions"] == [],
    "removal restricts the instrument, re-admission clears it")
req("MC-R24", all(any(j["type"] == "InstrumentRestrictionChanged" and j["restriction"] == "removed_instrument"
                      and j["reason"] == x["reason"] for j in x["expect"]["journal"])
                  for x in C["MC-R24"]["steps"] if x["event"] == "universe_changed"),
    "the restriction event names removed_instrument with the universe reason, not stale_mark")
s = steps("MC-R13")
req("MC-R13", s[2]["agent_equity"] == s[1]["agent_equity"] and s[2]["instrument_restrictions"] == [] and
    s[3]["instrument_restrictions"] == ["stale_mark"] and s[4]["instrument_restrictions"] == [] and s[5]["instrument_restrictions"] == ["stale_mark"],
    "pre-market ignored, failed mark stale, fresh clears, age 120 s stale")
s = steps("MC-R14")
req("MC-R14", s[1]["agent_mode"] == "paused" and s[3]["agent_mode"] == "exits_only" and s[4]["agent_mode"] == "normal", "daily flatten ack")
s = steps("MC-R15")
req("MC-R15", s[3]["agent_mode"] == "exits_only" and s[3]["restrictions"] == ["drawdown_exits_only"], "stricter holds")

s = steps("MC-R16")
req("MC-R16", s[0]["agent_mode"] == "exits_only" and s[1]["restrictions"] == ["goal_complete"] and "lifetime_floor" in s[2]["restrictions"],
    "disarmed ladder and daily; floor armed")
s = steps("MC-R17")
req("MC-R17", s[0]["agent_mode"] == "stopped" and any(j["type"] == "PositionReleased" for j in s[0]["journal"]), "released")


def released_with_carry(cid):
    """A release retires the agent: `AgentStopped` (reason goal_complete) right after `PositionReleased`, carrying
    max(0, N - E) recomputed from the case's own figures (DEC-270, MI-14)."""
    c = C[cid]
    x = [st for st in c["steps"] if st["event"] == "goal_complete"][-1]
    marks = [st["bid"] for st in c["steps"] if st["event"] == "mark"]
    mark = Decimal(marks[-1]) if marks else Decimal(c["initial"]["avg_cost"])
    allocation = Decimal(d["bases"][c["base"]]["mandate"]["capital"]["allocation_usd"])
    equity = allocation + Decimal(c["initial"]["position_qty"]) * (mark - Decimal(c["initial"]["avg_cost"]))
    carry = max(Decimal(0), allocation - equity)
    types = [j["type"] for j in x["expect"]["journal"]]
    stopped = [j for j in x["expect"]["journal"] if j["type"] == "AgentStopped"]
    req(cid, types[:3] == ["GoalCompleted", "PositionReleased", "AgentStopped"] and len(stopped) == 1
        and stopped[0]["reason"] == "goal_complete" and Decimal(stopped[0]["loss_carry_usd"]) == carry
        and Decimal(x["expect"]["agent_equity"]) == equity and x["expect"]["restrictions"] == ["retired"],
        f"release journals AgentStopped with the net dollar loss {carry}")
    return Decimal(stopped[0]["loss_carry_usd"]) if stopped else None


released_with_carry("MC-R17")
carry = released_with_carry("MC-R25")
req("MC-R25", carry is not None and carry > 0, "the release leaves a positive carry")
v68 = C["MC-V68"]
budget = (Decimal(d["bases"][v68["base"]]["mandate"]["capital"]["max_loss_from_allocation"])
          * Decimal(next(p["value"] for p in v68["patch"] if p["path"] == "/capital/allocation_usd")))
req("MC-V68", carry is not None and Decimal(v68["context"]["connection_loss_carry_usd"]) == carry and carry >= budget
    and v68["expect"]["violations"] == ["V-032"], "a redeploy is refused on the carry the release left (V-032)")
r26 = C["MC-R26"]
s = steps("MC-R26")
f26 = Decimal(d["bases"][r26["base"]]["mandate"]["capital"]["max_loss_from_allocation"])
c26, e26 = Decimal(s[1]["capital_base"]), Decimal(s[1]["agent_equity"])
req("MC-R26", carry is not None and Decimal(r26["initial"]["inherited_loss_usd"]) == carry
    and "lifetime_floor" not in s[0]["restrictions"] and "lifetime_floor" in s[1]["restrictions"]
    and c26 * (1 - f26) < e26 <= c26 * (1 - f26) + carry,
    "the redeploy's floor sits the carried L above C x (1 - f): it latches where a fresh connection's would not")
s = steps("MC-R18")
req("MC-R18", s[1]["restrictions"] == ["hard_breach"] and s[2]["agent_mode"] == "exits_only" and s[3]["agent_mode"] == "paused", "two-quote flatten")
s = steps("MC-R19")
req("MC-R19", s[1]["agent_mode"] == "exits_only" and s[2]["restrictions"] == [] and s[2]["agent_mode"] == "normal", "flash print latches nothing")
s = steps("MC-R20")
req("MC-R20", all("daily_loss" not in x["restrictions"] for x in s), "flash at midnight discarded")
s = steps("MC-R21")
req("MC-R21", s[2].get("error") == "waiting_period" and s[3].get("error") == "not_loosening" and "lifetime_floor" not in s[4]["restrictions"], "loosen path")
s = steps("MC-R22")
req("MC-R22", not any(j["type"] == "GoalCompleted" for x in s[:3] for j in x["journal"]) and any(j.get("reason") == "profit_stop_reached" for j in s[3]["journal"]), "profit stop")
s = steps("MC-R23")
req("MC-R23", any(j["type"] == "AgentStopped" and float(j["loss_carry_usd"]) > 849 for j in s[3]["journal"]), "carry keeps dollar loss")
# gate
for cid, reason in {"MC-G01": "concentration_limit", "MC-G03": "max_order_size", "MC-G04": "gross_exposure_limit",
                    "MC-G05": "concentration_limit", "MC-G06": "gross_exposure_limit", "MC-G07": "max_orders_per_day",
                    "MC-G11": "reentry_cooldown", "MC-G12": "reentry_cooldown",
                    "MC-G14": "not_in_working_universe"}.items():
    req(cid, C[cid]["expect"]["reason"] == reason, reason)
req("MC-G05", C["MC-G05"]["expect"]["computed"]["cap"] == "1425", "fraction cap binds")
req("MC-G06", C["MC-G06"]["expect"]["computed"]["gross_limit"] == "9500", "E caps gross")
req("MC-G16", C["MC-G16"]["expect"]["reason"] == "not_in_working_universe", "an empty universe denies")
for cid in ("MC-G02", "MC-G08", "MC-G09", "MC-G10", "MC-G13", "MC-G15"):
    req(cid, C[cid]["expect"]["verdict"] == "allow", "allow")

# builder
B = {cid: C[cid]["expect"] for cid in C if cid.startswith("MC-B")}

def calendar_at(now, asset_class):
    """Trading spec §4.3's session and §9.6's close window (15:50-16:00 ET on a full day) at `now`."""
    if asset_class == "crypto":
        return "crypto", False
    local = datetime.strptime(now[:19], "%Y-%m-%dT%H:%M:%S").replace(tzinfo=timezone.utc).astimezone(ZoneInfo("America/New_York"))
    t = local.time()
    if local.weekday() >= 5 or not time(4) <= t < time(20):
        return "overnight", False
    if t < time(9, 30):
        return "pre_market", False
    if t < time(16):
        return "regular", t >= time(15, 50)
    return "after_hours", False

for c in d["cases"]:
    if c["kind"] == "builder":
        inp = c["input"]
        session, window = calendar_at(inp["now"], inp["asset_class"])
        req(c["id"], inp.get("session", "regular") == session, f"`session` {inp.get('session', 'regular')} but `now` is {session}")
        req(c["id"], inp.get("in_close_window", False) == window, f"`in_close_window` {inp.get('in_close_window', False)} but `now` gives {window}")
req("MC-B01", B["MC-B01"]["action"] == "buy" and B["MC-B01"]["autonomy"]["decision"] == "auto", "auto open")
req("MC-B02", B["MC-B02"]["order_usd"] == "300", "factor halves")
req("MC-B03", B["MC-B03"]["reason"] == "between_thresholds", "hold")
req("MC-B04", B["MC-B04"]["purpose"] == "discretionary_exit" and B["MC-B04"]["autonomy"]["decision"] == "auto", "exit")
req("MC-B05", B["MC-B05"]["purpose"] == "increase", "increase")
req("MC-B06", B["MC-B06"]["action"] == "hold" and B["MC-B06"]["buy_conviction"] < B["MC-B06"]["combined_conviction"], "missing bearish for buys")
req("MC-B07", B["MC-B07"]["purpose"] == "discretionary_exit", "missing zero for exits")
req("MC-B08", B["MC-B08"]["autonomy"]["decision"] == "ask", "ask")
req("MC-B09", B["MC-B09"]["action"] == "buy", "future ignored")
req("MC-B10", B["MC-B10"]["outputs_used"] == ["llm.news_research"], "old ignored")
req("MC-B11", B["MC-B11"]["order_usd"] == B["MC-B01"]["order_usd"], "latest wins")
req("MC-B12", B["MC-B12"]["outputs_used"] == ["quant.momentum"], "wrong version ignored")
req("MC-B13", B["MC-B13"]["combined_score"] == "0.65" and B["MC-B13"]["autonomy"]["decision"] == "auto", "rounded then compared")
req("MC-B14", B["MC-B14"]["order_usd"] == "1000" and "limits" in B["MC-B14"]["clipped_by"], "clipped")
req("MC-B15", B["MC-B15"]["action"] == "hold", "working counted")
req("MC-B16", B["MC-B16"]["reason"] == "at_or_above_target", "no trim")
req("MC-B17", B["MC-B17"]["purpose"] == "risk_exit" and B["MC-B17"]["reason"] == "trim_to_target", "trim")
req("MC-B30", B["MC-B30"].get("trim_withheld") == ["rung_not_confirmed"], "trim waits")
req("MC-B31", set(B["MC-B31"].get("trim_withheld", [])) == {"holding", "regular_session_only"}, "trim guards")
req("MC-B32", "trim_withheld" not in B["MC-B32"] and B["MC-B32"].get("purpose") != "risk_exit", "below band, no trim")
BIN = {cid: C[cid]["input"] for cid in C if cid.startswith("MC-B")}
req("MC-B33", B["MC-B33"].get("purpose") == "risk_exit" and Decimal(B["MC-B33"]["qty"]) >= Decimal(BIN["MC-B33"]["min_order_size"])
    and Decimal(B["MC-B33"]["order_usd"]) < Decimal(BIN["MC-B33"]["min_order_usd"]), "a trim at least the minimum size goes under a larger dollar minimum")
def trim_sell(cid):
    """The trim the gate sizes (§5.5, DEC-399 item 7, DEC-445 item 2): the agent's resting sells come
    off the excess before it is rounded up on the instrument's quantity grid, a remainder beside them
    that is off the grid and is not the whole position is truncated onto it, and with none resting a
    trim past the position is the whole position, the full close DEC-423 exempts from the minimum."""
    inp, exp = BIN[cid], B[cid]
    inc, bid = Decimal(inp["qty_increment"]), Decimal(inp["quote"]["bid"])
    excess = Decimal(exp["current_mv"]) - Decimal(inp["size_factor"]) * Decimal(exp["cap"])
    on_sale = Decimal(inp["open_sell_qty"])
    held = Decimal(inp["position_qty"])
    unsold = max(Decimal(0), held - on_sale)
    owed = max(Decimal(0), excess - on_sale * bid)
    rounded = (owed / bid / inc).to_integral_value(rounding=ROUND_CEILING) * inc
    if rounded <= unsold:
        return rounded
    if unsold == held:
        return held
    return (unsold / inc).to_integral_value(rounding=ROUND_DOWN) * inc
req("MC-B34", B["MC-B34"].get("trim_withheld") == ["below_minimum_order"]
    and trim_sell("MC-B34") < Decimal(BIN["MC-B34"]["min_order_size"])
    and trim_sell("MC-B34") * Decimal(BIN["MC-B34"]["quote"]["bid"]) >= Decimal(BIN["MC-B34"]["min_order_usd"]),
    "a trim below the minimum size that the dollar minimum would send is withheld")
req("MC-B35", B["MC-B35"].get("purpose") == "risk_exit" and B["MC-B35"]["qty"] == BIN["MC-B35"]["position_qty"]
    and trim_sell("MC-B35") == Decimal(BIN["MC-B35"]["position_qty"])
    and Decimal(B["MC-B35"]["qty"]) < Decimal(BIN["MC-B35"]["min_order_size"]),
    "a trim of the whole position below the minimum size is a full close, and goes")
req("MC-B36", B["MC-B36"].get("trim_withheld") == ["below_minimum_order"] and Decimal(BIN["MC-B36"]["open_sell_qty"]) > 0
    and 0 < trim_sell("MC-B36") < Decimal(BIN["MC-B36"]["min_order_size"])
    and trim_sell("MC-B36") + Decimal(BIN["MC-B36"]["open_sell_qty"]) >= Decimal(BIN["MC-B36"]["min_order_size"]),
    "the remainder a resting sell leaves is below the minimum size, though the whole excess is not, and is withheld")
req("MC-B37", B["MC-B37"].get("purpose") == "risk_exit" and Decimal(BIN["MC-B37"]["open_sell_qty"]) > 0
    and Decimal(B["MC-B37"]["qty"]) == trim_sell("MC-B37") < trim_sell("MC-B37") + Decimal(BIN["MC-B37"]["open_sell_qty"]),
    "the trim is the remainder a resting sell leaves, not the whole excess")
b38g = Decimal(BIN["MC-B38"]["qty_increment"])
b38_unsold = Decimal(BIN["MC-B38"]["position_qty"]) - Decimal(BIN["MC-B38"]["open_sell_qty"])
b38_owed = max(Decimal(0), Decimal(B["MC-B38"]["current_mv"]) - Decimal(BIN["MC-B38"]["size_factor"]) * Decimal(B["MC-B38"]["cap"])
               - Decimal(BIN["MC-B38"]["open_sell_qty"]) * Decimal(BIN["MC-B38"]["quote"]["bid"]))
b38_up = (b38_owed / Decimal(BIN["MC-B38"]["quote"]["bid"]) / b38g).to_integral_value(rounding=ROUND_CEILING) * b38g
req("MC-B38", B["MC-B38"].get("purpose") == "risk_exit" and Decimal(BIN["MC-B38"]["open_sell_qty"]) > 0
    and b38_unsold % b38g != 0 and b38_up > b38_unsold
    and Decimal(B["MC-B38"]["qty"]) == (b38_unsold / b38g).to_integral_value(rounding=ROUND_DOWN) * b38g < b38_unsold
    and Decimal(B["MC-B38"]["qty"]) >= Decimal(BIN["MC-B38"]["min_order_size"]),
    "the unsold remainder is off the grid, the trim rounded up on it would sell more than it leaves, "
    "and the trim is the largest quantity on the grid at or below it")
b39g = Decimal(BIN["MC-B39"]["qty_increment"])
b39_bid = Decimal(BIN["MC-B39"]["quote"]["bid"])
b39_excess = Decimal(B["MC-B39"]["current_mv"]) - Decimal(BIN["MC-B39"]["size_factor"]) * Decimal(B["MC-B39"]["cap"])
b39_on_sale = Decimal(BIN["MC-B39"]["open_sell_qty"])
b39_after = (max(Decimal(0), b39_excess - b39_on_sale * b39_bid) / b39_bid / b39g).to_integral_value(rounding=ROUND_CEILING) * b39g
b39_first = (b39_excess / b39_bid / b39g).to_integral_value(rounding=ROUND_CEILING) * b39g - b39_on_sale
req("MC-B39", B["MC-B39"].get("purpose") == "risk_exit" and b39_on_sale > 0
    and Decimal(B["MC-B39"]["qty"]) == b39_after < b39_first
    and Decimal(B["MC-B39"]["qty"]) <= Decimal(BIN["MC-B39"]["position_qty"]) - b39_on_sale
    and Decimal(B["MC-B39"]["qty"]) >= Decimal(BIN["MC-B39"]["min_order_size"]),
    "the trim rounds up what the resting sells leave of the excess, which rounding the whole excess up "
    "first would size larger, and sells no more of the position than they leave unsold")
req("MC-B18", B["MC-B18"]["reason"] == "within_rebalance_band", "band")
req("MC-B19", B["MC-B19"]["reason"] == "below_band_after_clipping", "band after clipping")
req("MC-B20", B["MC-B20"]["reason"] == "no_fresh_outputs", "none")
req("MC-B21", B["MC-B21"]["autonomy"]["decision"] == "skipped", "dry run")
req("MC-B22", B["MC-B22"]["gate_dry_run"]["verdict"] == "defer", "defer session")
req("MC-B23", B["MC-B23"]["gate_dry_run"]["verdict"] == "allow" and B["MC-B23"].get("order_type") == "marketable_limit", "exit in window")
req("MC-B24", B["MC-B24"]["autonomy"]["decision"] == "deny", "averaging down denied")
req("MC-B25", B["MC-B25"]["autonomy"]["decision"] == "auto", "re-entry, no averaging rule hit")
req("MC-B26", "goal" in B["MC-B26"]["clipped_by"] and B["MC-B26"]["qty"] == "0.01", "target clip")
req("MC-B27", "goal" in B["MC-B27"]["clipped_by"] and B["MC-B27"]["qty"] == "0.005", "avg clip")
req("MC-B28", B["MC-B28"]["qty"] != "0.01" and "goal" in B["MC-B28"]["clipped_by"], "fees change the clip")
req("MC-B29", B["MC-B29"]["reason"] == "discretionary_exits_disabled", "no sells")
# autonomy
for cid, dec in {"MC-A01": "auto", "MC-A02": "auto", "MC-A03": "auto", "MC-A04": "auto", "MC-A05": "ask", "MC-A06": "ask",
                 "MC-A07": "auto", "MC-A08": "auto", "MC-A09": "ask", "MC-A10": "ask", "MC-A11": "ask",
                 "MC-A12": "ask", "MC-A13": "auto", "MC-A14": "deny", "MC-A15": "deny", "MC-A16": "ask"}.items():
    req(cid, C[cid]["expect"]["decision"] == dec, dec)
req("MC-A10", C["MC-A10"]["expect"]["approvers_required"] == 2, "two approvers")
req("MC-A12", C["MC-A12"]["expect"]["by"] == "admission_ceiling", "the ceiling, not the rule, decided")
req("MC-A13", C["MC-A13"]["expect"]["by"].startswith("rule:"), "the rule decides once the owner allows auto")
req("MC-A15", C["MC-A15"]["expect"]["by"] == "rule:no_new", "the ceiling never loosens a deny rule")
# flatten
F = {cid: C[cid]["expect"] for cid in ("MC-F01", "MC-F02", "MC-F03", "MC-F04")}
for cid, f in F.items():
    req(cid, f["cancel_client_order_ids"] == ["a-1", "a-2"] and not f["cancel_all_endpoint"] and not f["close_position_endpoint"], "agent scope")
req("MC-F02", len(F["MC-F02"]["deferred_sells"]) == 1, "equity deferred")
req("MC-F03", F["MC-F03"]["deferred_sells"] == [] and F["MC-F03"]["purpose"] == "owner_exit"
    and F["MC-F03"]["sells"][0].get("floor_price") == "97", "owner sells now with a floor")
req("MC-F04", len(F["MC-F04"]["deferred_sells"]) == 1, "owner unconfirmed waits")
# goals
req("MC-L05", not C["MC-L05"]["expect"]["done"], "not done")
for cid in ("MC-L01", "MC-L02", "MC-L03", "MC-L04"):
    req(cid, C[cid]["expect"]["done"] and C[cid]["expect"]["then"] == "hold_protected", "done, on_complete")
# research agent: admission (§8.5), lineage (§8.6), expiry (DEC-118), stagger (DEC-100)
ADM = {cid: C[cid]["expect"] for cid in C if C[cid]["kind"] == "admission"}
adm_reason = {"MC-N25": "leveraged_etp_not_enabled",
              "MC-N02": "universe_full", "MC-N03": "not_allowed_asset_class", "MC-N04": "eligibility_floor",
              "MC-N05": "instrument_group_claimed", "MC-N06": "no_corroboration", "MC-N07": "source_not_allowlisted",
              "MC-N08": "research_disabled", "MC-N09": "cost_cap_reached", "MC-N10": "operator_halt",
              "MC-N11": "not_in_data_universe", "MC-N12": "direction_not_allowed", "MC-N13": "horizon_mismatch",
              "MC-N15": "admission_denied", "MC-N16": "leveraged_etp_not_enabled"}
for cid, reason in adm_reason.items():
    req(cid, ADM[cid]["admitted"] is False and ADM[cid]["reason"] == reason, f"refused with {reason}")
    req(cid, not any(j["type"] == "UniverseChanged" for j in ADM[cid]["journal"]), "a refusal changes no universe")
    req(cid, ADM[cid]["first_order_autonomy"] is None, "a refusal proposes no order")
for cid in ("MC-N12", "MC-N13", "MC-N19"):
    e = ADM.get(cid) or C[cid]["expect"]["steps"][0]
    req(cid, e.get("ignored", e.get("reason") == "revision_without_predecessor"), "a malformed thesis is ignored")
req("MC-N26", ADM["MC-N26"]["admitted"] and ADM["MC-N26"]["reason"] is None,
    "a leveraged ETP with the opt-in and the accepted disclosure is admitted")
req("MC-N01", ADM["MC-N01"]["admitted"] and ADM["MC-N01"]["change"] == "admitted"
    and ADM["MC-N01"]["universe_size_after"] == 1
    and any(j["type"] == "UniverseChanged" and j["change"] == "admitted" for j in ADM["MC-N01"]["journal"]), "admitted")
req("MC-N01", ADM["MC-N01"]["first_order_autonomy"]["decision"] == "ask", "the platform default for admissions is ask")
req("MC-N02", ADM["MC-N02"]["universe_size_after"] == 5, "a full universe is unchanged")
req("MC-N14", ADM["MC-N14"]["change"] == "renewed" and ADM["MC-N14"]["universe_size_after"] == 1
    and not any(j["type"] == "UniverseChanged" for j in ADM["MC-N14"]["journal"]), "a renewal adds no entry")
for cid in ADM:
    kinds = {j["type"] for j in ADM[cid]["journal"]}
    req(cid, ("ThesisRevised" in kinds) == (C[cid]["input"]["thesis"]["revision"] > 0), "revision journals ThesisRevised")

L17 = C["MC-N17"]["expect"]["steps"]
req("MC-N17", [s["admitted"] for s in L17] == [True, True, True, True, False], "cap 3 admits three revisions")
req("MC-N17", L17[-1]["reason"] == "lineage_retired" and C["MC-N17"]["expect"]["lineages"]["th-20"]["retired"],
    "the fourth revision retires the lineage")
req("MC-N18", all(not s["score_carried_forward"] for s in C["MC-N18"]["expect"]["steps"]), "no score carried forward")
req("MC-N19", C["MC-N19"]["expect"]["steps"][0]["reason"] == "revision_without_predecessor", "lineage needs a predecessor")
L24 = C["MC-N24"]["expect"]
req("MC-N24", L24["steps"][-1]["reason"] == "lineage_retired" and L24["steps"][-1]["lineage_retired"],
    "the over-cap revision retires the lineage")
req("MC-N24", L24["working_universe"] == [] and L24["steps"][-1]["universe_size_after"] == 0
    and any(j["type"] == "UniverseChanged" and j["reason"] == "lineage_retired" for j in L24["steps"][-1]["journal"]),
    "retirement removes the instrument the lineage held, journalled from the fold")

req("MC-N20", C["MC-N20"]["expect"]["journal"][0]["reason"] == "thesis_expired"
    and C["MC-N20"]["expect"]["working_universe"] == [], "the horizon removes the instrument")
req("MC-N21", C["MC-N21"]["expect"]["journal"][0]["reason"] == "thesis_invalidated", "invalidation removes at once")
N27 = C["MC-N27"]["expect"]["steps"][-1]
req("MC-N27", N27["reason"] == "direction_not_allowed" and not N27["lineage_retired"]
    and not any(j["type"] == "UniverseChanged" for j in N27["journal"]),
    "an earlier check's refusal retires nothing and removes nothing")
req("MC-N27", C["MC-N27"]["expect"]["working_universe"] != [], "the instrument stays")
N28 = C["MC-N28"]["expect"]
req("MC-N28", N28["steps"][-1]["reason"] == "lineage_retired" and N28["steps"][-1]["lineage_retired"]
    and not any(j["type"] == "UniverseChanged" for j in N28["steps"][-1]["journal"])
    and len(N28["working_universe"]) == 1,
    "retirement leaves an instrument another lineage holds")
req("MC-N28", list(N28["lineage_instruments"]) == ["th-80"], "the holder moved to the renewing lineage")
req("MC-C48", C["MC-C48"]["expect"]["classification"] == "risk_increasing",
    "pinning a no-agent mandate is increasing (round-1 finding 4)")
req("MC-N22", len(C["MC-N22"]["expect"]["removed"]) == 1 and len(C["MC-N22"]["expect"]["working_universe"]) == 1,
    "only the retired lineage is removed")
req("MC-N22", "lineages" in C["MC-N22"]["input"] and not any("lineage_retired" in e for e in C["MC-N22"]["input"]["entries"]),
    "retirement is read from the lineage state, never a per-entry flag")
for cid in ("MC-N20", "MC-N21", "MC-N22"):
    e = C[cid]["expect"]
    req(cid, all(v == "removed_instrument" for v in e["instrument_restrictions"].values()),
        "removal restricts only that instrument")

S23 = C["MC-N23"]["expect"]
req("MC-N23", all(0 <= o < S23["window_s"] for o in S23["offsets"]), "offsets inside the window")
req("MC-N23", S23["offsets"][0] != S23["offsets"][1] and S23["offsets"][0] != S23["offsets"][2],
    "different workspaces and theses stagger differently")

# the review date (§6.2 step 5b, MI-32, V-046; DEC-188)
for cid, code in {"MC-D02": "V-020", "MC-D05": "V-046", "MC-D06": "V-046", "MC-D07": "V-015", "MC-D09": "V-046",
                  "MC-D10": "V-046", "MC-D12": "V-042"}.items():
    req(cid, C[cid]["expect"]["violations"] == [code], f"expected exactly {code}")
req("MC-D01", C["MC-D01"]["context"]["provenance"]["/autonomy/review_by"]["source"] == "platform_default"
    and C["MC-D01"]["patch"][0]["value"] == "2026-12-23" and d["validation_context_defaults"]["validation_date"] == "2026-09-24",
    "the platform default is the validation date + 90 days")
req("MC-D08", C["MC-D08"]["patch"][0]["value"] == C["MC-D08"]["context"]["previous_version"]["autonomy"]["review_by"]
    < d["validation_context_defaults"]["validation_date"], "a lapsed date, carried unchanged")
req("MC-D09", C["MC-D09"]["patch"] == [] and "review_by" in C["MC-D09"]["context"]["previous_version"]["autonomy"], "removed")
req("MC-D11", C["MC-D11"]["context"]["previous_version"]["autonomy"]["delegations"][0]["id"]
    == next(p["value"][0]["id"] for p in C["MC-D11"]["patch"] if p["path"] == "/autonomy/delegations"), "the same delegation id")
for cid, cls in {"MC-D13": "risk_reducing", "MC-D14": "risk_reducing", "MC-D15": "risk_increasing", "MC-D16": "risk_increasing"}.items():
    req(cid, C[cid]["expect"]["classification"] == cls and C[cid]["expect"]["changed_paths"] == ["/autonomy/review_by"], cls)
req("MC-D15", C["MC-D15"]["expect"]["step_up_required"], "re-confirming needs step-up")
NYT = ZoneInfo("America/New_York")
def review_local(cid):
    return datetime.strptime(C[cid]["now"][:19], "%Y-%m-%dT%H:%M:%S").replace(tzinfo=timezone.utc).astimezone(NYT)
REVIEW = d["bases"]["btc_accumulator_reviewed"]["mandate"]["autonomy"]["review_by"]
req("MC-D17", review_local("MC-D17").date().isoformat() == REVIEW and review_local("MC-D17").time() == time(23, 59, 59),
    "the last second of the review date in New York")
for cid in ("MC-D18", "MC-D19", "MC-D20", "MC-D21", "MC-D22", "MC-D24", "MC-D25", "MC-D26"):
    req(cid, review_local(cid).date().isoformat() > REVIEW and review_local(cid).time() == time(0, 0), "00:00 New York after it")
for cid, (dec, by) in {"MC-D17": ("auto", "rule:routine"), "MC-D18": ("ask", "review_ceiling"), "MC-D19": ("ask", "rule:large_orders"),
                       "MC-D20": ("deny", "rule:deny_big"), "MC-D21": ("ask", "review_ceiling"), "MC-D22": ("ask", "review_ceiling"),
                       "MC-D23": ("auto", "delegation:d1"), "MC-D24": ("ask", "review_ceiling"),
                       "MC-D25": ("auto", "builtin_risk_reducing"), "MC-D26": ("auto", "builtin_risk_reducing"),
                       "MC-D27": ("auto", "rule:routine")}.items():
    req(cid, (C[cid]["expect"]["decision"], C[cid]["expect"]["by"]) == (dec, by), f"{dec} by {by}")
req("MC-D21", C["MC-D21"]["patch"][1]["value"] == "auto", "an auto default")
req("MC-D22", C["MC-D22"]["action"]["new_instrument"] and C["MC-D22"]["patch"][0]["value"] == "auto", "an auto admission")
for cid in ("MC-D18", "MC-D19", "MC-D21", "MC-D22", "MC-D24"):
    tr = C[cid]["expect"]["trigger"]
    req(cid, tr["decided_by"] == C[cid]["expect"]["by"] and tr["requested_by"] == "agent" and tr["client"] is None, "the trigger names what asked")
    req(cid, (tr["rule"] is None) == (C[cid]["expect"]["by"] == "review_ceiling"), "a review-ceiling ask shows no rule; a rule's ask shows it")
req("MC-D19", C["MC-D19"]["expect"]["trigger"]["rule"]["id"] == "large_orders", "the owner's rule verbatim")
req("MC-D23", C["MC-D23"]["now"] == C["MC-D17"]["now"] and C["MC-D24"]["now"] == C["MC-D18"]["now"]
    and C["MC-D23"]["patch"] == C["MC-D24"]["patch"] and C["MC-D23"]["action"] == C["MC-D24"]["action"], "the same delegation either side")
req("MC-D27", "review_by" not in d["bases"]["btc_accumulator"]["mandate"]["autonomy"] and C["MC-D27"]["now"] > C["MC-D18"]["now"],
    "no review date, long after")

# change
inc = {"MC-C01", "MC-C02", "MC-C04", "MC-C08", "MC-C10", "MC-C12", "MC-C14", "MC-C15", "MC-C17", "MC-C18", "MC-C19", "MC-C20",
       "MC-C21", "MC-C22", "MC-C24", "MC-C25", "MC-C26", "MC-C29", "MC-C30", "MC-C31", "MC-C33", "MC-C35",
       "MC-C37", "MC-C38", "MC-C40", "MC-C42", "MC-C43", "MC-C47"}
red = {"MC-C03", "MC-C05", "MC-C06", "MC-C11", "MC-C13", "MC-C16", "MC-C27", "MC-C28", "MC-C32",
       "MC-C36", "MC-C39", "MC-C41", "MC-C44", "MC-C45", "MC-C46"}
neu = {"MC-C07", "MC-C09", "MC-C23"}
for cid in inc:
    req(cid, C[cid]["expect"]["classification"] == "risk_increasing", "increasing")
for cid in red:
    req(cid, C[cid]["expect"]["classification"] == "risk_reducing", "reducing")
for cid in neu:
    req(cid, C[cid]["expect"]["classification"] == "neutral", "neutral")
req("MC-C34", C["MC-C34"]["expect"]["classification"] == "invalid", "invalid")
# risk days
req("MC-T02", C["MC-T02"]["expect"]["length_s"] == 82800, "23 h")
req("MC-T04", C["MC-T04"]["expect"]["length_s"] == 90000, "25 h")

# escalation (§6.1, §6.4): each title's outcome, read from the drafts of the step that judges it
def esc(cid):
    return [[(x["type"], x.get("result"), x.get("reason")) for x in st["drafts"]] for st in C[cid]["expect"]]

def esc_last(cid):
    return esc(cid)[-1]

ACTED = [("ApprovalResponded", "admitted", None), ("ApprovalRevalidated", "act", None), ("IntentProposed", None, None)]
def refused(reason):
    return [("ApprovalResponded", "refused", reason)]
def skipped(reason):
    return [("ApprovalResponded", "admitted", None), ("ApprovalRevalidated", "skip", reason)]

for cid in ("MC-E01", "MC-E21", "MC-E24", "MC-E29"):
    req(cid, esc_last(cid) == ACTED, "acts")
for cid in sorted(c for c in C if c.startswith("MC-E") and C[c]["op"] == "lifecycle"):
    first = esc(cid)[0]
    req(cid, [t for t, _, _ in first] == ["ApprovalRequested", "ApprovalDelivered"], "the request is delivered in its own step")
    for st in C[cid]["expect"]:
        for x in st["drafts"]:
            if x["type"] == "IntentProposed":
                b = C[cid]["script"][0]["bound"]
                req(
                    cid,
                    x["instrument_id"] == b["instrument"]
                    and x["side"] == b["side"]
                    and x["order_type"] == "limit"
                    and x["tif"] == ("day" if b["asset_class"] == "us_equity" else "gtc")
                    and all(x[k] == b[k] for k in ("qty", "limit_price", "purpose")),
                    "the bound order",
                )
        if not any(x["type"] == "ApprovalRevalidated" and x["result"] == "act" for x in st["drafts"]):
            req(cid, all(x["type"] != "IntentProposed" for x in st["drafts"]), "only an act proposes")
req("MC-E01", C["MC-E01"]["expect"][1]["drafts"][0]["verdict"] == "approved", "a grant")
req("MC-E02", esc_last("MC-E02") == [("ApprovalResponded", "admitted", None)] and
    C["MC-E02"]["script"][1]["response"]["step_up"] is None, "an admitted skip without step-up, nothing re-validated")
req("MC-E03", esc("MC-E03")[1] == [("ApprovalTimedOut", None, None)] and esc_last("MC-E03") == refused("not_pending"), "timeout, then not pending")
req("MC-E03", C["MC-E03"]["expect"][1]["drafts"][0]["on_timeout"] == "skip", "the default is a skip")
req("MC-E04", esc_last("MC-E04") == refused("late") and
    C["MC-E04"]["script"][1]["response"]["submitted_at"] == C["MC-E04"]["expect"][0]["drafts"][0]["deadline"], "submitted at the deadline")
req("MC-E05", esc_last("MC-E05") == refused("late") and
    C["MC-E05"]["script"][2]["response"]["submitted_at"] < C["MC-E05"]["expect"][0]["drafts"][0]["deadline"], "early submission, late clock")
req("MC-E06", esc("MC-E06")[1] == ACTED and esc_last("MC-E06") == [], "copied and acted once")
for cid, reason in [("MC-E07", "content_mismatch"), ("MC-E10", "not_an_approver"), ("MC-E11", "not_an_approver"),
                    ("MC-E12", "not_an_approver"), ("MC-E13", "step_up_missing"), ("MC-E14", "step_up_stale"),
                    ("MC-E16", "step_up_method")]:
    req(cid, esc_last(cid) == refused(reason), reason)
req("MC-E08", esc("MC-E08")[1] == [("ApprovalCanceled", None, "version_applied")] and esc_last("MC-E08") == refused("not_pending"), "cancelled")
req("MC-E09", esc("MC-E09")[1:] == [refused("not_an_approver")] * 2 and
    [s["response"]["actor_kind"] for s in C["MC-E09"]["script"][1:]] == ["agent", "system"], "agent and system")
req("MC-E10", C["MC-E10"]["script"][1]["response"]["actor_kind"] == "broker", "broker")
req("MC-E11", C["MC-E11"]["script"][1]["response"]["actor_kind"] == "platform_operator", "platform_operator")
req("MC-E12", C["MC-E12"]["script"][1]["response"]["responder"] not in C["MC-E12"]["context"]["approvers"], "not listed")
req("MC-E15", esc_last("MC-E15") == refused("step_up_reused"), "reused")
req("MC-E16", C["MC-E16"]["context"]["environment"] == "live", "live")
for cid, reason in [("MC-E17", "version_changed"), ("MC-E19", "reclassified_deny"),
                    ("MC-E20", "reclassified_other_trigger"), ("MC-E22", "drift"), ("MC-E23", "drift")]:
    req(cid, esc_last(cid) == skipped(reason), reason)
for cid, ok in [("MC-E21", True), ("MC-E22", False)]:
    now = C[cid]["script"][1]["now"]["mark"]
    ref_ = C[cid]["script"][0]["bound"]["reference_mark"]["price"]
    req(cid, (abs(Decimal(now) - Decimal(ref_)) * 10000 <= 100 * Decimal(ref_)) == ok and Decimal(now) != Decimal(ref_), "the band edge")
req("MC-E22", Decimal(C["MC-E22"]["script"][1]["now"]["mark"]) < Decimal("155"), "a falling price")
req("MC-E23", C["MC-E23"]["script"][0]["bound"]["reference_mark"] is None, "no reference mark")
m24 = (Decimal(C["MC-E24"]["script"][1]["now"]["mark"]) / Decimal("50000") - 1) * 10000
req("MC-E24", C["MC-E24"]["script"][0]["bound"]["asset_class"] == "crypto" and 100 < m24 <= 200, "between the two bands")
req("MC-E25", [q["suppressed"] for q in C["MC-E25"]["expect"]] == ["budget", None] and len(C["MC-E25"]["ledger"]) == 10, "budget then a new day")
req("MC-E26", [q["suppressed"] for q in C["MC-E26"]["expect"]] == ["budget", None], "resets at New York midnight")
req("MC-E26", all(q["at"][:10] == "2026-11-01" for q in C["MC-E26"]["queries"]), "both queries on one UTC day")
req("MC-E27", [q["suppressed"] for q in C["MC-E27"]["expect"]] == ["skipped_today", None, None], "until the next day, this instrument only")
req("MC-E28", [q["suppressed"] for q in C["MC-E28"]["expect"]] == ["recent_timeout", None], "one timeout_s")
local = datetime.fromisoformat(C["MC-E29"]["start"][:19] + "+00:00").astimezone(ZoneInfo("America/New_York"))
req("MC-E29", local.hour == 2 and C["MC-E29"]["context"]["quiet_hours"] is not None, "02:00 New York under quiet hours")
req("MC-E30", [q["status"] for q in C["MC-E30"]["expect"]] ==
    ["suppressed_quiet_hours", "delivered", "suppressed_quiet_hours", "delivered", "delivered"], "push hours, both DST states")
offsets = {datetime.fromisoformat(q["at"][:19] + "+00:00").astimezone(ZoneInfo("America/New_York")).utcoffset() for q in C["MC-E30"]["queries"]}
req("MC-E30", len(offsets) == 2, "both DST states")
req("MC-E18", esc_last("MC-E18") == [("ApprovalCanceled", None, "mode_tightened")] + refused("not_pending") and
    C["MC-E18"]["script"][-1]["kind"] == "response" and C["MC-E18"]["script"][-1]["now"]["mode"] == "exits_only",
    "an exits-only step cancels before it judges the grant (DEC-318 option (a))")
req("MC-E31", esc_last("MC-E31") == [("ApprovalCanceled", None, "mode_tightened")] + refused("not_pending"), "cancelled first, in one step")
# MC-E32: each query is inside quiet hours by one zone and outside by the other, so the expected
# statuses are the New York reading and the negation of the UTC one. A UTC implementation fails it
# whatever the DST state (the #401 review, minor).
def qh_inside(hour, minute):
    start, end = 23 * 60, 7 * 60
    return ((hour * 60 + minute) - start) % 1440 < (end - start) % 1440

ny = ZoneInfo("America/New_York")
for q, e in zip(C["MC-E32"]["queries"], C["MC-E32"]["expect"]):
    utc = datetime.fromisoformat(q["at"][:19] + "+00:00")
    local = utc.astimezone(ny)
    by_ny, by_utc = qh_inside(local.hour, local.minute), qh_inside(utc.hour, utc.minute)
    req("MC-E32", by_ny != by_utc, f"{q['at']} reads the same in both zones")
    req("MC-E32", e["status"] == ("suppressed_quiet_hours" if by_ny else "delivered"), "the New York reading")
req("MC-E32", len({datetime.fromisoformat(q["at"][:19] + "+00:00").astimezone(ny).utcoffset()
                   for q in C["MC-E32"]["queries"]}) == 2, "both DST states")
req("MC-E32", {e["status"] for e in C["MC-E32"]["expect"]} == {"delivered", "suppressed_quiet_hours"}, "both answers")
req("MC-E", sum(c.startswith("MC-E") for c in C) == 32, "32 escalation cases")

# tripwires (§6.7, MI-31, V-044; DEC-187, DEC-350 to DEC-352)
for cid, code in {"MC-W07": "V-044", "MC-W08": "V-044", "MC-W09": "V-044", "MC-W10": "V-044", "MC-W12": "V-044",
                  "MC-W13": "V-044", "MC-W16": "V-020", "MC-W17": "V-042"}.items():
    req(cid, C[cid]["expect"]["violations"] == [code], f"expected exactly {code}")
req("MC-W02", C["MC-W02"]["patch"][0]["value"][0]["action"] == "paused", "the paused action")
req("MC-W05", len(C["MC-W05"]["patch"][0]["value"]) == 21, "21 tripwires")
for cid, cls in {"MC-W19": "risk_reducing", "MC-W20": "risk_reducing", "MC-W21": "risk_reducing", "MC-W22": "risk_increasing",
                 "MC-W23": "risk_increasing", "MC-W24": "risk_increasing", "MC-W25": "risk_increasing", "MC-W26": "risk_increasing"}.items():
    e = C[cid]["expect"]
    req(cid, e["classification"] == cls and e["changed_paths"] == ["/autonomy/tripwires"]
        and e["step_up_required"] == (cls == "risk_increasing"), cls)

def tw_fired_at(cid):
    """The step indices at which each tripwire fired."""
    return [(i, j["limit"]) for i, x in enumerate(C[cid]["expect"]) for j in x["journal"] if j["type"] == "RiskLimitTriggered"]

def tw_events(cid, i):
    return [j["type"] for j in C[cid]["expect"][i]["journal"]]

def tw_state(cid, i=-1):
    return C[cid]["expect"][i]["state"]

def probe(cid, purpose, usd=None):
    return next(p["expect"] for p in C[cid]["probes"]
                if p["action"]["purpose"] == purpose and (usd is None or p["action"].get("order_usd") == usd))

for cid in [c for c in C if c.startswith("MC-W") and C[c]["kind"] == "tripwire"]:
    c = C[cid]
    req(cid, len(c["steps"]) == len(c["expect"]) and c["steps"][0]["event"] == "MandateVersionApplied", "deployed first, one expectation per step")
    for x in c["expect"]:
        for k, j in enumerate(x["journal"]):
            if j["type"] == "RiskLimitTriggered":
                req(cid, k + 1 < len(x["journal"]) and x["journal"][k + 1] == {"type": "OwnerAlertSent", "subject": f"RiskLimitTriggered:{k}",
                                                                                 "text": "tripwire_fired"}, "each firing alerts, opaquely")
        req(cid, x["state"]["restriction"] in (None, "exits_only"), "never paused")
        req(cid, x["state"]["delegations_suspended"] == bool(x["state"]["fired"]), "fired exactly when suspended")
req("MC-W27", tw_fired_at("MC-W27") == [(3, "tripwire:losing_streak")] and tw_state("MC-W27", 2)["metrics"]["losing_streak"] == "1",
    "the second losing exit, not the first")
req("MC-W27", tw_state("MC-W27")["restriction"] == "exits_only" and probe("MC-W27", "open")["by"] == "rule:large_orders", "exits only, no delegation")
for cid in ("MC-W27", "MC-W30"):
    req(cid, all(probe(cid, p) == {"decision": "auto", "by": "builtin_risk_reducing"} for p in ("discretionary_exit", "owner_exit", "risk_exit")),
        "exits stay built-in AUTO")
req("MC-W28", tw_fired_at("MC-W28") == [] and [x["state"]["metrics"]["losing_streak"] for x in C["MC-W28"]["expect"]][2:] == ["1", "0", "1"],
    "the winning exit resets the streak")
req("MC-W29", tw_fired_at("MC-W29") == [(4, "tripwire:losing_streak")] and tw_state("MC-W29", 3)["metrics"]["losing_streak"] == "1"
    and C["MC-W29"]["steps"][3]["side"] == "buy" and C["MC-W29"]["steps"][2]["price"] == "100", "a buy keeps the streak; an exit at cost loses its fee")
req("MC-W30", tw_fired_at("MC-W30") == [(3, "tripwire:day_loss")] and tw_state("MC-W30")["restriction"] is None
    and probe("MC-W30", "open", "950")["by"] == "rule:large_orders" and probe("MC-W30", "open", "300")["by"] == "rule:routine",
    "delegations end, the mode stays normal, auto rules still decide")
req("MC-W31", tw_fired_at("MC-W31") == [] and tw_state("MC-W31")["metrics"]["day_loss"] == "99.99"
    and probe("MC-W31", "open", "950")["by"] == "delegation:d1", "one cent short; the delegation lifts")
req("MC-W32", tw_fired_at("MC-W32") == [] and tw_state("MC-W32", 3)["metrics"]["day_loss"] == "0" and C["MC-W32"]["steps"][3]["event"] == "RiskDayStarted"
    and tw_state("MC-W32")["metrics"]["day_loss"] == "60", "the new risk day counts afresh")
req("MC-W33", tw_fired_at("MC-W33") == [(2, "tripwire:day_loss")] and all(s.get("side") in (None, "buy") for s in C["MC-W33"]["steps"]),
    "commissions alone")
req("MC-W34", tw_fired_at("MC-W34") == [(4, "tripwire:new_names")] and tw_state("MC-W34", 3)["metrics"]["new_names"] == "1", "re-entry does not count")
req("MC-W35", tw_fired_at("MC-W35") == [] and tw_state("MC-W35", 3)["metrics"] == {} and tw_state("MC-W35")["metrics"]["losing_streak"] == "1",
    "nothing before arming counts")
req("MC-W36", tw_fired_at("MC-W36") == [(4, "tripwire:losing_streak")] and C["MC-W36"]["steps"][4]["event"] == "MandateVersionApplied",
    "fires at the version's own input")
req("MC-W37", tw_state("MC-W37", 4)["fired"] == {"losing_streak": "exits_only"} and tw_state("MC-W37", 4)["metrics"] == {}
    and tw_events("MC-W37", 5) == ["RiskLimitLifted"] and tw_state("MC-W37")["fired"] == {}, "removal keeps it; the acknowledgment lifts")
req("MC-W38", C["MC-W38"]["expect"][4]["journal"] == [{"type": "OwnerCommandRefused", "command": "acknowledge", "reason": "step_up_stale"}]
    and tw_state("MC-W38")["fired"] == {"losing_streak": "exits_only"}, "stale step-up refused")
req("MC-W39", tw_events("MC-W39", 1) == [] and C["MC-W39"]["expect"][5]["journal"][0]["reason"] == "step_up_reused"
    and tw_state("MC-W39", 5)["fired"] != {} and tw_events("MC-W39", 6) == ["RiskLimitLifted"]
    and tw_state("MC-W39")["metrics"]["losing_streak"] == "1" and tw_state("MC-W39")["fired"] == {}, "reused refused; fresh lifts and counts afresh")
req("MC-W40", [i for i, _ in tw_fired_at("MC-W40")] == [3, 6] and tw_events("MC-W40", 4) == ["RiskLimitLifted"], "fires again after two more")
req("MC-W41", tw_state("MC-W41", 2) == dict(tw_state("MC-W41", 2), restriction=None) and tw_state("MC-W41")["restriction"] == "exits_only"
    and tw_events("MC-W41", 3) == [], "tightened while fired, held at once without firing again")
req("MC-W42", tw_state("MC-W42", 4)["fired"] == {"losing_streak": "exits_only"} and tw_state("MC-W42")["restriction"] is None, "softening keeps exits_only")
req("MC-W43", [l for _, l in tw_fired_at("MC-W43")] == ["tripwire:day_loss", "tripwire:losing_streak"]
    and tw_events("MC-W43", 2) == ["RiskLimitTriggered", "OwnerAlertSent"] * 2, "two in id order on one fill")
req("MC-W44", tw_state("MC-W44", 3)["metrics"]["losing_streak"] == "2" and tw_state("MC-W44", 4)["metrics"]["losing_streak"] == "0"
    and tw_fired_at("MC-W44") == [(6, "tripwire:losing_streak")], "the new metric counts afresh")
req("MC-W45", tw_events("MC-W45", 3) == [] and tw_state("MC-W45", 3)["metrics"]["losing_streak"] == "1"
    and tw_fired_at("MC-W45") == [(4, "tripwire:losing_streak")], "an acknowledgment of nothing changes nothing")
req("MC-W46", tw_fired_at("MC-W46") == [(3, "tripwire:losing_streak")] and C["MC-W46"]["steps"][3]["event"] == "LateFillApplied", "a late fill")
req("MC-W47", tw_fired_at("MC-W47") == [] and tw_state("MC-W47")["metrics"]["losing_streak"] == "0"
    and Decimal(C["MC-W47"]["steps"][2]["price"]) - Decimal(C["MC-W47"]["steps"][1]["price"]) == Decimal("1e-12"),
    "the tie rounds to even: the exit breaks even")
req("MC-W48", tw_fired_at("MC-W48") == [(4, "tripwire:losing_streak")] and C["MC-W48"]["steps"][3]["event"] == "RiskDayStarted"
    and tw_state("MC-W48", 3)["metrics"]["losing_streak"] == "1", "the streak survives midnight")
req("MC-W49", C["MC-W49"]["expect"][4]["journal"][0]["reason"] == "step_up_method"
    and C["MC-W49"]["expect"][5]["journal"][0]["reason"] == "step_up_reused"
    and C["MC-W49"]["steps"][4]["step_up"]["assertion"] == C["MC-W49"]["steps"][5]["step_up"]["assertion"]
    and tw_state("MC-W49")["fired"] == {"losing_streak": "exits_only"}, "a refused acknowledgment spends its assertion")
req("MC-W50", C["MC-W50"]["expect"][4]["journal"] == [{"type": "OwnerCommandRefused", "command": "acknowledge", "reason": "not_independent"}]
    and C["MC-W50"]["steps"][4]["user"] == C["MC-W50"]["steps"][4]["requester"] and tw_state("MC-W50")["fired"] != {},
    "the requester cannot lift it under independent approval")
req("MC-W51", tw_events("MC-W51", 4) == ["RiskLimitLifted"] and C["MC-W51"]["steps"][4]["user"] != C["MC-W51"]["steps"][4]["requester"]
    and C["MC-W51"]["steps"][4]["independent_approval_required"], "a second user lifts it")
w52 = [s["expect"] for s in C["MC-W52"]["steps"]]
req("MC-W52", C["MC-W52"]["kind"] == "risk_state" and "tripwire" in w52[1]["restrictions"] and w52[1]["agent_mode"] == "exits_only"
    and w52[2].get("error") == "increase_blocked_while_latched" and w52[3]["journal"][0]["type"] == "RiskLimitLifted"
    and w52[3]["agent_mode"] == "normal" and "error" not in w52[4], "MI-7 while fired; the increase applies after the acknowledgment")
for cid, who in (("MC-W53", "requester"), ("MC-W54", "user")):
    req(cid, C[cid]["steps"][4][who] is None and C[cid]["expect"][4]["journal"][0]["reason"] == "not_independent"
        and tw_state(cid)["fired"] != {}, f"no {who} named: refused, fail closed")
req("MC-W55", C["MC-W55"]["steps"][4]["independent_approval_required"] and not C["MC-W55"]["steps"][4]["independent_now"]
    and C["MC-W55"]["expect"][4]["journal"][0]["reason"] == "not_independent", "bound at the request")
req("MC-W56", not C["MC-W56"]["steps"][4]["independent_approval_required"] and C["MC-W56"]["steps"][4]["independent_now"]
    and C["MC-W56"]["expect"][4]["journal"][0]["reason"] == "not_independent", "raised at processing")
w57 = [s["expect"] for s in C["MC-W57"]["steps"]]
req("MC-W57", C["MC-W57"]["kind"] == "risk_state" and C["MC-W57"]["steps"][1]["event"] == "risk_day_started"
    and [j for j in w57[2]["journal"] if j["type"] == "RiskLimitTriggered"][0]["action"] == "end_delegations"
    and "tripwire" not in w57[2]["restrictions"] and w57[3].get("error") == "increase_blocked_while_latched",
    "an end_delegations tripwire latches; the loss counts from the new risk day")
req("MC-W", sum(c.startswith("MC-W") for c in C) == 57, "57 tripwire cases")

# delegation routing (§9.2, MI-29; #444, DEC-353)
J_INC = {"MC-J01", "MC-J03", "MC-J05"}
for k in range(1, 11):
    cid = f"MC-J{k:02}"
    cls = "risk_increasing" if cid in J_INC else "risk_reducing"
    req(cid, C[cid]["expect"]["classification"] == cls and C[cid]["expect"]["changed_paths"] == ["/autonomy/rules"], cls)
au_of = lambda cid: d["bases"][C[cid]["base"]]["mandate"]["autonomy"]
lifts = lambda cid: [x["lifts"] for x in au_of(cid).get("delegations", [])]
bare = lambda cid: {k: v for k, v in d["bases"][C[cid]["base"]]["mandate"].items() if k != "autonomy"} | {
    "autonomy": {k: v for k, v in au_of(cid).items() if k != "delegations"}}
for deleg, ctrl, lift in [("MC-J01", "MC-J02", "rule:large_orders"), ("MC-J03", "MC-J04", "rule:low_score"), ("MC-J05", "MC-J08", "default"),
                          ("MC-J06", "MC-J07", "rule:low_score"), ("MC-J09", "MC-J10", "rule:low_score")]:
    req(deleg, lifts(deleg) == [lift] and lifts(ctrl) == [] and bare(deleg) == bare(ctrl) and C[deleg]["patch"] == C[ctrl]["patch"],
        f"the same change as {ctrl}, whose base differs only by the delegation")
req("MC-J06", au_of("MC-J06")["rules"][0]["then"] == "auto" and C["MC-J06"]["patch"] == [{"op": "remove", "path": "/autonomy/rules/0"}],
    "an auto rule removed")
req("MC-J09", au_of("MC-J09")["rules"][0]["then"] == "auto" and au_of("MC-J09")["rules"][0]["when"]["op"] == "lt"
    and int(C["MC-J09"]["patch"][0]["value"]) < int(au_of("MC-J09")["rules"][0]["when"]["value"]), "an auto rule narrowed")
req("MC-J", sum(c.startswith("MC-J") for c in C) == 10, "10 routing cases")
# DEC-539: the offset is required where the connection's profile protects with a stop-limit, and W-002 adds it there.
SL = {"stop_limit_asset_classes": ["crypto", "us_equity"]}
req("MC-K05", C["MC-K05"]["context"] == SL and C["MC-K05"]["expect"]["violations"] == ["V-008"], "equities need the offset")
req("MC-K06", C["MC-K06"]["context"] == SL
    and Decimal(C["MC-K06"]["expect"]["worst_case"]["one_position_at_stop_usd"])
    > Decimal(C["MC-K07"]["expect"]["worst_case"]["one_position_at_stop_usd"]), "the worst case adds the offset")
PAPER = {"stop_limit_asset_classes": ["crypto"]}
ABSENT = {"stop_limit_asset_classes": None}
req("MC-K", d["validation_context_defaults"]["stop_limit_asset_classes"] == ["crypto"],
    "the defaults state the bases' Alpaca profile, so no case relies on an absent one")
req("MC-K07", C["MC-K07"]["context"] == PAPER and C["MC-K07"]["expect"]["violations"] == [], "the Alpaca profile needs none")
req("MC-K08", C["MC-K08"]["context"] == SL and C["MC-K08"]["expect"]["worst_case"]
    == C["MC-K06"]["expect"]["worst_case"], "version 1's field reads as version 2's")
req("MC-K10", C["MC-K10"]["expect"]["changed_paths"] == ["/protection/stop_limit_offset"], "only the offset changed")
req("MC-K11", C["MC-K11"]["expect"]["classification"] == "neutral" and C["MC-K11"]["expect"]["changed_paths"] == [],
    "a move to version 2 alone changes nothing")
req("MC-K12", C["MC-K12"]["context"] == ABSENT and C["MC-K12"]["expect"]["violations"] == ["V-008"]
    and C["MC-K07"]["patch"] == C["MC-K12"]["patch"], "absent, the profile fails closed: the equity needs the offset")
req("MC-K13", C["MC-K13"]["context"] == ABSENT and C["MC-K13"]["expect"]["warnings"] == ["W-002"]
    and Decimal(C["MC-K13"]["expect"]["worst_case"]["one_position_at_stop_usd"])
    > Decimal(C["MC-K13"]["expect"]["worst_case"]["daily_loss_budget_usd"]), "absent, the worst case adds the offset")
req("MC-K14", C["MC-K14"]["context"] == PAPER and C["MC-K14"]["patch"] == C["MC-K13"]["patch"]
    and C["MC-K14"]["expect"]["warnings"] == []
    and Decimal(C["MC-K14"]["expect"]["worst_case"]["one_position_at_stop_usd"])
    < Decimal(C["MC-K13"]["expect"]["worst_case"]["one_position_at_stop_usd"]), "the Alpaca profile leaves it out")
req("MC-K15", C["MC-K15"]["context"]["previous_version"]["mandate_schema_version"] == 2
    and C["MC-K15"]["expect"]["violations"] == ["V-031"], "no move back to version 1")
req("MC-K", sum(c.startswith("MC-K") for c in C) == 15, "15 stop-limit offset cases")
# §11 states how many cases the file holds; nothing else compares that prose with the file (#530 review, m2).
SPEC = (pathlib.Path(__file__).resolve().parents[2] / "docs/specs/mandate.md").read_text()
stated = re.findall(r"signal-model registry, and (\d+) cases that implementations must", SPEC)
req("§11", stated == [str(len(d["cases"]))], f"§11 states {stated} cases, the file holds {len(d['cases'])}")
print("cases", len(C), "title assertion failures", len(bad))
for b in bad:
    print(" ", b)
sys.exit(1 if bad else 0)
