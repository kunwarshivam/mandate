"""Asserts that every reference case demonstrates what its title claims (AGENTS.md: validate fixtures)."""
import pathlib
import sys
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
        req(cid, e["schema_valid"] == (cid == "MC-S01"), "only S01 is valid")
    if k == "semantic":
        code = {"V-01": None}
        t = c["title"].lower()
        passes = any(w in t for w in ("passes", "exactly equal", "with the accepted", "is fine", "platform defaults on listed",
                                      "user-entered and confirmed", "with two approvers", "equal to the validation",
                                      "below the floor budget", "is a warning", "shadowed", "(warning w-003)"))
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
       "MC-V64": "V-022", "MC-V67": "V-020"}
for cid, code in exp.items():
    req(cid, C[cid]["expect"]["violations"] == [code], f"expected exactly {code}")
req("MC-V13", "W-003" in C["MC-V13"]["expect"]["warnings"], "W-003")
req("MC-V46", "W-001" in C["MC-V46"]["expect"]["warnings"], "W-001")
req("MC-V47", "W-005" in C["MC-V47"]["expect"]["warnings"], "W-005")
req("MC-V48", len(C["MC-V48"]["expect"]["violations"]) >= 3, "multiple")
req("MC-V60", C["MC-V60"]["expect"]["violations"] == ["V-003", "V-036"], "accumulate admits nothing, two ways")
req("MC-V65", "W-006" in C["MC-V65"]["expect"]["warnings"], "W-006")

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

print("cases", len(C), "title assertion failures", len(bad))
for b in bad:
    print(" ", b)
sys.exit(1 if bad else 0)
