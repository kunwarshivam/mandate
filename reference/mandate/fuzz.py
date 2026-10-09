"""Property-based fuzz of the reference implementation against the invariants of mandate spec §1.1 (MI-1 to MI-32)."""
import copy, itertools, json, random, sys
from collections import Counter
from fractions import Fraction
from datetime import date, datetime, timedelta, timezone
from zoneinfo import ZoneInfo
from ref import *  # noqa: F401,F403
from ref import D
import bases as base

rng = random.Random(int(sys.argv[1]) if len(sys.argv) > 1 else 7)
FAIL = []

def check(ok, name, ctx):
    if not ok:
        FAIL.append((name, ctx))

def rand_mandate(cls):
    m = copy.deepcopy(base.swing if cls == "us_equity" else base.btc)
    r = m["risk"]
    r.update({"max_position_usd": "10000", "max_position_fraction": "1", "max_gross_exposure_usd": "10000"})
    r["breach_confirm_s"] = rng.choice([0, 30, 60, 300])
    r["scale_lift_after_s"] = rng.choice([0, 300, 600])
    r["daily_breach_min_s"] = rng.choice([0, 1800, 3600])
    r["daily_loss_action"] = rng.choice(["exits_only", "flatten_and_pause"])
    r["max_daily_loss"] = rng.choice(["0.02", "0.05", "0.1"])
    m["capital"]["max_loss_from_allocation"] = rng.choice(["0.08", "0.1", "0.2"])
    if cls == "crypto":
        m["goal"]["on_complete"] = rng.choice(["hold_protected", "disarm_ladder", "release"])
    if rng.random() < 0.5:
        r["drawdown_ladder"] = [{"at": "0.02", "action": "scale_sizes", "factor": "0.75"},
                                {"at": "0.04", "action": "scale_sizes", "factor": "0.5"},
                                {"at": "0.06", "action": "exits_only", "factor": None},
                                {"at": "0.08", "action": "flatten_and_pause", "factor": None}]
    V.validate(m)
    return m

def rand_steps(cls, n, start):
    t = T(start)
    steps, qty = [], D(100 if cls == "us_equity" else "0.1")
    px = D(100 if cls == "us_equity" else 60000)
    day = t.date()
    for _ in range(n):
        t += timedelta(seconds=rng.choice([1, 15, 30, 59, 60, 120, 900, 3600, 5 * 3600]))
        if t.astimezone(NY).date() != day:
            day = t.astimezone(NY).date()
            midnight = datetime(day.year, day.month, day.day, tzinfo=NY).astimezone(timezone.utc)
            steps.append({"event": "risk_day_started", "at": fmt(midnight), "session": "crypto" if cls == "crypto" else "after_hours"})
            if midnight > t:
                t = midnight
        local = t.astimezone(NY)
        sess = "crypto" if cls == "crypto" else ("regular" if 9 * 60 + 30 <= local.hour * 60 + local.minute < 16 * 60 else "after_hours")
        k = rng.random()
        at = fmt(t)
        if k < 0.6:
            px = max(px * (1 + D(rng.choice([-5, -3, -2, -1, -0.5, 0, 0.5, 1, 2, 3])) / 100), D(1))
            steps.append({"event": "mark", "at": at, "bid": norm(px.quantize(D("0.01"))), "session": sess,
                          "sane": rng.random() > 0.05})
        elif k < 0.75:
            steps.append({"event": "clock", "at": at, "session": sess})
        elif k < 0.8:
            steps.append({"event": "allocation_change", "at": at, "session": sess,
                          "delta_usd": rng.choice(["-2000", "-500", "500", "3000"])})
        elif k < 0.88:
            steps.append({"event": "owner_acknowledged", "at": at, "session": sess,
                          "restriction": rng.choice(["drawdown_ladder", "daily_loss", "lifetime_floor"])})
        elif k < 0.95 and qty > 0:
            q = qty if rng.random() < 0.5 else (qty / 2).quantize(D("0.0001"))
            qty -= q
            steps.append({"event": "fill", "at": at, "session": sess, "side": "sell", "qty": norm(q), "price": norm(px.quantize(D("0.01")))})
        elif rng.random() < 0.1:
            steps.append({"event": "goal_complete", "at": at, "session": sess})
        elif rng.random() < 0.05:
            steps.append({"event": "agent_stopped", "at": at, "session": sess})
        elif rng.random() < 0.1:
            steps.append({"event": "floor_loosened", "at": at, "session": sess, "new_max_loss_from_allocation": rng.choice(["0.15", "0.3", "0.5"]),
                          "independent_approval": rng.random() < 0.5, "confirmed_at": fmt(t - timedelta(hours=rng.choice([1, 30, 60])))})
        else:
            q = D(10 if cls == "us_equity" else "0.01")
            qty += q
            steps.append({"event": "fill", "at": at, "session": sess, "side": "buy", "qty": norm(q), "price": norm(px.quantize(D("0.01")))})
    return steps

class Oracle:
    """Independent breach-time accumulator for MI-4 (§5.6): left-continuous, reset after need s false."""
    def __init__(self, need):
        self.need, self.acc, self.false_run, self.prev, self.hard_t = need, 0.0, 0.0, False, None

    def feed(self, breached, hard, dt, t, quote):
        if self.prev:
            self.acc += dt
        else:
            self.false_run += dt
            if self.false_run >= self.need:
                self.acc = 0.0
        if breached:
            self.false_run = 0.0
        self.prev = breached
        must = breached and self.acc >= self.need
        if quote and hard:
            if self.hard_t is None:
                self.hard_t = t
            elif (t - self.hard_t).total_seconds() >= min(self.need, 10):
                must = True
        elif quote:
            self.hard_t = None
        return must

def run(m, cls, steps, start, L="0"):
    rs = RiskState(m, "100" if cls == "us_equity" else "0.1", "100" if cls == "us_equity" else "60000", cls, start, inherited_loss=L)
    outs = []
    D_net = D(m["capital"]["allocation_usd"])
    oracle = Oracle(rs.need)
    f_oracle = Oracle(rs.need)
    last_daily_trigger, day_after, acked = None, False, False
    prev_t = T(start)
    for s in steps:
        before = copy.deepcopy((rs.restrictions, {k: v["active"] for k, v in rs.scale.items()}, dict(rs.latched), rs.daily))
        pre = (rs.equity(), rs.H, rs.E0, rs.C, rs.L, rs.f)
        qty_before = rs.qty
        o = rs.step(s)
        outs.append(o)
        types = [e["type"] for e in o["journal"]]
        t = T(s["at"])
        # MI-2
        if s["event"] == "allocation_change" and "error" not in o:
            D_net += D(s["delta_usd"])
            after = types[types.index("MandateVersionApplied") + 1:]
            check(not any(x in ("RiskLimitTriggered", "RiskLimitLifted", "KillSwitchActivated", "AgentModeApplied") for x in after),
                  "MI-2 no trigger or lift on allocation change", (s, o))
            E, H, E0, C, Lp, f = pre
            E1, H1, E01, C1 = D(o["agent_equity"]), D(o["high_water_mark"]), D(o["day_start_equity"]), D(o["capital_base"])
            check((H1 - E1) / H1 >= (H - E) / H, "MI-2 drawdown not lowered", (s, o))
            check((E1 - E01) / E01 <= (E - E0) / E0, "MI-2 daily loss fraction not lowered", (s, o))
            check((E1 - C1) / C1 <= (E - C) / C, "MI-2 return not raised", (s, o))
            check((E1 - C1 * (1 - f) - rs.L) / E1 <= (E - C * (1 - f) - Lp) / E, "MI-2 floor headroom not raised", (s, o))
        # MI-3
        if s["event"] == "risk_day_started" and last_daily_trigger is not None:
            day_after = True
        if s["event"] == "owner_acknowledged" and s["restriction"] == "daily_loss" and "error" not in o:
            acked = True
        removed = set(before[0]) - set(rs.restrictions)
        for rname in removed:
            if rname in ("drawdown_exits_only", "drawdown_flatten"):
                check(s["event"] == "owner_acknowledged" and s["restriction"] == "drawdown_ladder", "MI-3 drawdown latch lifted by ack only", (s, o))
                if rname == "drawdown_flatten":
                    check(qty_before == 0, "MI-3 flatten acknowledged only when flat", (s, o))
            if rname == "lifetime_floor":
                check(s["event"] == "floor_loosened" and "error" not in o, "MI-3 floor lifts only by a loosening version", (s, o))
            if rname == "daily_loss":
                ok = last_daily_trigger is not None and day_after and (t - last_daily_trigger).total_seconds() >= rs.daily_min \
                    and (rs.daily_action == "exits_only" or acked)
                check(ok, "MI-3 daily lifts after a new day, the minimum delay, and acknowledgment", (s, o))
        for e in o["journal"]:
            if e["type"] == "RiskLimitTriggered" and e.get("limit") == "max_daily_loss" and e.get("reason") != "hard_breach_pending":
                last_daily_trigger, day_after, acked = t, False, False
        if s["event"] == "owner_acknowledged" and s["restriction"] == "daily_loss" and "error" not in o:
            acked = True
        # MI-4 with an independent oracle (the floor must latch whenever the oracle requires it)
        E, C = D(o["agent_equity"]), D(o["capital_base"])
        quote = s["event"] in ("mark", "fill") and s.get("sane", True) and not (s["event"] == "mark" and cls == "us_equity" and s["session"] != "regular")
        dt_ = (t - prev_t).total_seconds()
        if s["event"] == "floor_loosened" and "error" not in o:
            oracle = Oracle(rs.need)
        elif "lifetime_floor" not in before[2]:
            must = oracle.feed(E <= C * (1 - rs.f) + rs.L, E <= C * (1 - D("1.25") * rs.f) + rs.L, dt_, t, quote)
            latched_now = "lifetime_floor" in rs.restrictions
            check(latched_now == must, "MI-4 floor latches exactly when the oracle says so", (s, o, must))
        # the same two-sided oracle for the flatten rung (no latch on one quote, no missed latch)
        fi = len(rs.lad) - 1
        fkey = f"drawdown_ladder[{fi}]"
        at_f = D(rs.lad[fi]["at"])
        Hn = D(o["high_water_mark"])
        if s["event"] == "owner_acknowledged" and "error" not in o and s["restriction"] == "drawdown_ladder":
            f_oracle = Oracle(rs.need)
        elif fkey not in before[2] and not rs.disarmed:
            must_f = f_oracle.feed(Hn - E >= at_f * Hn, Hn - E >= D("1.25") * at_f * Hn, dt_, t, quote)
            check((fkey in rs.latched) == must_f, "MI-4 flatten rung latches exactly when the oracle says so", (s, o, must_f))
        prev_t = t
        # MI-5
        H = D(o["high_water_mark"])
        check(H >= E and D(0) <= (H - E) / H < 1, "MI-5 H >= E", (s, o))
        # MI-6
        strictest = max(rs.restrictions.values(), key=lambda x: SEVERITY[x], default="normal")
        check(o["agent_mode"] == strictest, "MI-6 mode is strictest", (s, o))
        changed = max(before[0].values(), key=lambda x: SEVERITY[x], default="normal") != strictest
        check(("AgentModeApplied" in types) == changed, "MI-6 AgentModeApplied iff change", (s, o))
        # MI-7 (latched derived from restrictions, not from the implementation's own predicate)
        if s["event"] == "allocation_change" and D(s["delta_usd"]) > 0 and any(x in before[0] for x in LATCHES):
            check(o.get("error") == "increase_blocked_while_latched", "MI-7 increase blocked while latched", (s, o))
        # MI-14: the loss carried at retirement is the net dollar loss, whatever withdrawals came first
        for e in o["journal"]:
            if e["type"] == "AgentStopped":
                check(D(e["loss_carry_usd"]) == max(D(0), D_net - E), "MI-14 loss carry is the net dollar loss", (s, o))
        # §5.8: sizes step back from the highest active scale rung, so the active ones are always the shallowest
        active = {D(rs.lad[i]["at"]) for i, st in rs.scale.items() if st["active"]}
        check(all(D(rs.lad[i]["at"]) in active for i in rs.scale if active and D(rs.lad[i]["at"]) < max(active)),
              "§5.8 the active scale rungs are the shallowest", (s, o))
        # §3.1: a completed profit_stop goal's one outcome is the discretionary exit, then retirement
        for e in o["journal"]:
            if e["type"] == "GoalCompleted" and m["goal"]["type"] == "profit_stop":
                check(e.get("then") == "discretionary_exit_all_then_retire" and "on_complete" not in e,
                      "§3.1 a profit_stop goal completes with its one outcome", (s, o))
        if s["event"] == "goal_complete" and m["goal"].get("on_complete") == "release" and "retired" not in before[0]:
            RELEASES[0] += 1
        # MI-14 and DEC-270: every retirement, a release included, journals the carry once; a redeploy on the
        # connection is refused (V-032) or opens at that L, so its floor sits the released loss above C x (1 - f)
        if "retired" in rs.restrictions and "retired" not in before[0]:
            stops = [e for e in o["journal"] if e["type"] == "AgentStopped"]
            check(len(stops) == 1, "MI-14 a retirement journals AgentStopped once, release included", (s, o))
            if stops:
                carry = max(D(0), D_net - E)
                budget = D(m["capital"]["max_loss_from_allocation"]) * D(m["capital"]["allocation_usd"])
                again, _ = semantic(m, dict(base.CTX, connection_loss_carry_usd=stops[0]["loss_carry_usd"]))
                check(("V-032" in again) == (carry >= budget), "MI-14 a redeploy is refused exactly when the carry uses the floor budget", (s, o))
                fresh = RiskState(m, "0", "1", cls, s["at"], inherited_loss=stops[0]["loss_carry_usd"])
                Cn = D(m["capital"]["allocation_usd"])
                floor = Cn * (1 - D(m["capital"]["max_loss_from_allocation"])) + carry
                check(fresh.conditions(E=floor, H=Cn, E0=Cn, C=Cn)["lifetime_floor"][0]
                      and not fresh.conditions(E=floor + D("0.01"), H=Cn, E0=Cn, C=Cn)["lifetime_floor"][0],
                      "MI-14 a release never lets a redeploy reset the lifetime floor", (s, o, floor))
    return outs

RELEASES = [0]

def fuzz_risk(n_runs):
    for k in range(n_runs):
        cls = rng.choice(["us_equity", "crypto"])
        m = rand_mandate(cls)
        start = "2026-09-21T13:35:00.000000000Z"
        steps = rand_steps(cls, rng.randint(20, 80), start)
        L = rng.choice(["0", "0", "300"])
        a = run(m, cls, steps, start, L)
        noisy = json.loads(json.dumps(steps))
        for st_ in noisy:
            st_["event_time"] = fmt(T(st_["at"]) + timedelta(seconds=rng.randint(-300, 300)))
        b = run(m, cls, noisy, start, L)
        check(a == b, "MI-8 replay identical (JSON round trip, event_time ignored)", k)
        # MI-13: dropping clock ticks that emitted no events changes no other output
        keep = [i for i, st in enumerate(steps) if not (st["event"] == "clock" and not a[i]["journal"])]
        c = run(m, cls, [steps[i] for i in keep], start, L)
        strip = lambda o: {x: y for x, y in o.items() if x != "pending"}
        check([strip(a[i]) for i in keep] == [strip(x) for x in c], "MI-13 no-event ticks are replay-safe", k)
    check(RELEASES[0] > 0, "the fuzz exercised a release (DEC-270)", RELEASES[0])

def fuzz_stepped_lift(n_runs):
    """§5.8's stepped lift, which the general walk rarely reaches: latch the exits_only rung, acknowledge, then wander
    across the scale rungs' trigger and lift levels so a lifted rung can trigger again while the others step back."""
    start = "2026-09-21T13:35:00.000000000Z"
    for k in range(n_runs):
        m = rand_mandate("crypto")
        m["risk"]["drawdown_ladder"] = [{"at": "0.02", "action": "scale_sizes", "factor": "0.75"},
                                        {"at": "0.04", "action": "scale_sizes", "factor": "0.5"},
                                        {"at": "0.06", "action": "exits_only", "factor": None},
                                        {"at": "0.08", "action": "flatten_and_pause", "factor": None}]
        m["risk"]["breach_confirm_s"] = 0
        m["risk"]["max_daily_loss"] = "0.5"
        m["risk"]["scale_lift_after_s"] = rng.choice([60, 300, 600])
        m["capital"]["max_loss_from_allocation"] = "0.5"
        V.validate(m)
        t = T(start) + timedelta(seconds=60)
        steps = [{"event": "mark", "at": fmt(t), "bid": "53000", "session": "crypto", "sane": True}]
        t += timedelta(seconds=60)
        steps.append({"event": "owner_acknowledged", "at": fmt(t), "session": "crypto", "restriction": "drawdown_ladder"})
        for _ in range(rng.randint(10, 40)):
            t += timedelta(seconds=rng.choice([30, 60, 120, 300, 600, 900]))
            if rng.random() < 0.25:
                steps.append({"event": "clock", "at": fmt(t), "session": "crypto"})
            else:
                steps.append({"event": "mark", "at": fmt(t), "bid": str(rng.choice(range(48500, 53600, 100))),
                              "session": "crypto", "sane": True})
        run(m, "crypto", steps, start)

def fuzz_gate(n):
    m = copy.deepcopy(base.swing)
    for _ in range(n):
        st = {"now": "2026-09-22T15:00:00.000000000Z", "agent_equity": norm(D(rng.randint(5000, 12000))),
              "positions_mv": {base.XYZ: norm(D(rng.randint(0, 3000)))}, "working_opening_orders": [],
              "orders_today": rng.randint(0, 60), "last_exit_fill_at": {base.XYZ: "2026-09-22T14:59:00.000000000Z"},
              "working_universe": rng.choice([[], [base.XYZ], [base.QRS, base.XYZ]])}
        mode = rng.choice(["normal", "exits_only"])
        inst = set(rng.sample(["stale_mark", "removed_instrument"], rng.randint(0, 2)))
        session = rng.choice(["regular", "pre_market", "after_hours"])
        window = rng.random() < 0.3
        for purpose in ("risk_exit", "protective", "owner_exit", "discretionary_exit"):
            confirmed = rng.random() < 0.5
            g = order_decision(m, st, {"instrument": base.XYZ, "purpose": purpose, "qty": str(rng.randint(1, 500)), "limit_price": "100"},
                               mode, inst, session, window, owner_confirmed_bid=confirmed)
            if purpose == "discretionary_exit":
                check(g["verdict"] in ("allow", "defer"), "MI-1 discretionary exits allowed or deferred", (purpose, mode, session, window, g))
            elif purpose == "owner_exit" and session != "regular" and not confirmed:
                check(g["verdict"] == "defer", "MI-1 unconfirmed owner exit waits", (purpose, g))
            else:
                check(g["verdict"] == "allow", "MI-1 exits allowed", (purpose, mode, session, window, inst, g))

def fuzz_gate_universe(n):
    """The working universe is a required gate input: an absent key fails closed, never open (rule 3)."""
    m = copy.deepcopy(base.swing)
    for _ in range(n):
        st = {"now": "2026-09-22T15:00:00.000000000Z", "agent_equity": "10000",
              "positions_mv": {base.XYZ: "0"}, "working_opening_orders": [], "orders_today": 0,
              "last_exit_fill_at": {}}
        prop = {"instrument": rng.choice([base.XYZ, base.QRS]), "purpose": rng.choice(["open", "increase"]),
                "qty": str(rng.randint(1, 5)), "limit_price": "100"}
        raised = False
        try:
            g = gate(m, st, prop)
        except KeyError:
            raised = True
        check(raised or (g["verdict"] == "deny" and g["reason"] == "not_in_working_universe"),
              "MI-15 an absent working universe never allows an opening", (st, prop))
        g2 = gate(m, dict(st, working_universe=[]), prop)
        check(g2["verdict"] == "deny" and g2["reason"] == "not_in_working_universe",
              "MI-15 an empty working universe denies every opening", (prop, g2))
        g3 = gate(m, dict(st, working_universe=[prop["instrument"]]), prop)
        check(g3["reason"] != "not_in_working_universe",
              "MI-15 an instrument in the working universe passes the check", (prop, g3))

def rand_outputs(m, inst):
    outs = []
    for sm in m["behavior"]["signal_models"]:
        if rng.random() < 0.85:
            outs.append({"model_id": sm["id"], "model_version": sm["version"], "content_hash": sm["content_hash"],
                         "instrument_id": inst, "as_of": "2026-09-22T13:59:00.000000000Z", "expires_at": "2026-09-22T15:00:00.000000000Z",
                         "conviction": norm(D(rng.randint(-100, 100)) / 100), "confidence": norm(D(rng.randint(0, 100)) / 100)})
    return outs

def fuzz_builder(n):
    m = copy.deepcopy(base.swing)
    for _ in range(n):
        E = D(rng.randint(8000, 12000))
        qty = D(rng.randint(0, 20))
        work = D(rng.choice([0, 0, 200, 700]))
        mv = qty * D("99.9")
        inp = {"now": "2026-09-22T14:00:00.000000000Z", "instrument": base.XYZ, "asset_class": "us_equity",
               "agent_equity": norm(E), "position_qty": norm(qty), "quote": {"bid": "99.9", "ask": "100"},
               "qty_increment": "1", "min_order_usd": "1", "size_factor": rng.choice(["1", "0.5"]),
               "working_opening_orders": [{"instrument": base.XYZ, "max_cost": norm(work)}] if work else [],
               "gate_state": {"agent_equity": norm(E), "positions_mv": {base.XYZ: norm(mv)},
                              "working_opening_orders": [{"instrument": base.XYZ, "max_cost": norm(work)}] if work else [],
                              "orders_today": 0, "last_exit_fill_at": {}, "working_universe": [base.XYZ]},
               "outputs": rand_outputs(m, base.XYZ)}
        o = builder(m, inp)
        if o.get("action") == "buy":
            check(o["gate_dry_run"]["reason"] not in ("concentration_limit", "max_order_size", "gross_exposure_limit"),
                  "MI-9 builder respects size limits", (inp, o))
        for drop in range(len(inp["outputs"])):
            inp2 = dict(inp, outputs=[x for i, x in enumerate(inp["outputs"]) if i != drop])
            o2 = builder(m, inp2)
            v1 = D(o["order_usd"]) if o.get("action") == "buy" else D(0)
            v2 = D(o2["order_usd"]) if o2.get("action") == "buy" else D(0)
            check(v2 <= v1, "MI-10 dropping a model never enlarges a buy", (inp, o, o2))

FIELDS_NUM = {"order_usd": ["100", "500", "900", "950", "5000"], "combined_score": ["0.5", "0.6", "0.65", "0.7", "0.9"],
              "bought_today_usd": ["0", "1500", "2500"], "position_pnl_fraction": ["-0.1", "0", "0.05"]}

def rand_rule(i):
    f = rng.choice(list(FIELDS_NUM) + ["purpose", "session"])
    if f == "purpose":
        w = {"field": "purpose", "op": "in", "value": sorted(rng.sample(["open", "increase"], rng.randint(1, 2)))}
    elif f == "session":
        w = {"field": "session", "op": rng.choice(["eq", "ne"]), "value": rng.choice(["regular", "crypto"])}
    else:
        w = {"field": f, "op": rng.choice(["gt", "gte", "lt", "lte"]), "value": rng.choice(FIELDS_NUM[f])}
    return {"id": f"r{i}", "when": w, "then": rng.choice(["auto", "ask", "deny"])}

def mutate(au):
    n = copy.deepcopy(au)
    k = rng.random()
    rules = n["rules"]
    if k < 0.25 and rules:
        r = rng.choice(rules)
        w = r["when"]
        if w["field"] in FIELDS_NUM:
            w["value"] = rng.choice(FIELDS_NUM[w["field"]])
    elif k < 0.45 and rules:
        rng.choice(rules)["then"] = rng.choice(["auto", "ask", "deny"])
    elif k < 0.6:
        rules.insert(rng.randint(0, len(rules)), rand_rule(len(rules) + 10))
    elif k < 0.75 and rules:
        rules.pop(rng.randrange(len(rules)))
    elif k < 0.85:
        n["default"] = rng.choice(["ask", "deny", "auto"])
    elif len(rules) > 1:
        i = rng.randrange(len(rules) - 1)
        rules[i], rules[i + 1] = rules[i + 1], rules[i]
    return n

def fuzz_pinning(n):
    """DEC-121 and MI-16: a change classified reducing never widens what the agent may open."""
    for _ in range(n):
        old = rand_research_mandate()
        if old["universe"]["pinned"]:
            continue
        if rng.random() < 0.5:
            old["behavior"]["research"] = None          # a valid unpinned mandate with no research agent
            for sm in old["behavior"]["signal_models"]:
                sm["admits_instruments"] = False
            if not V.is_valid(old) or semantic(old, dict(base.CTX, disclosures_accepted=["sha256:" + "b" * 64]))[0]:
                continue
        new = copy.deepcopy(old)
        nu = new["universe"]
        nu["pinned"] = True
        picks = rng.randint(1, 3)
        nu["pinned_instruments"] = sorted(
            ({"asset_id": base.XYZ, "symbol": "XYZ", "asset_class": "us_equity"},
             {"asset_id": base.QRS, "symbol": "QRS", "asset_class": "us_equity"},
             {"asset_id": base.LMN, "symbol": "LMN", "asset_class": "us_equity"})[:picks],
            key=lambda i: i["asset_id"])
        nu["asset_classes"] = ["us_equity"]
        nu["max_instruments"] = max(picks, rng.choice([picks, old["universe"]["max_instruments"]]))
        new["protection"]["crypto_stop_limit_offset"] = None
        new["behavior"]["research"] = None
        for sm in new["behavior"]["signal_models"]:
            sm["admits_instruments"] = False
        if not V.is_valid(new) or semantic(new, dict(base.CTX, disclosures_accepted=["sha256:" + "b" * 64]))[0]:
            continue
        cls, _ = classify(old, new)
        had_agent = any(s["admits_instruments"] for s in old["behavior"]["signal_models"])
        if cls == "risk_reducing":
            check(had_agent, "MI-16 pinning is reducing only when it turns a research agent off", (old["universe"], nu))
            check(nu["max_instruments"] <= old["universe"]["max_instruments"],
                  "MI-16 a reducing pin never raises max_instruments", (old["universe"], nu))

def fuzz_autonomy(n):
    m = copy.deepcopy(base.btc)
    for _ in range(n):
        m["autonomy"]["rules"] = [rand_rule(i) for i in range(rng.randint(0, 4))]
        m["autonomy"]["default"] = rng.choice(["ask", "deny", "auto"])
        m["autonomy"]["admission"] = rng.choice(["ask", "deny", "auto"])
        new = copy.deepcopy(m)
        new["autonomy"] = mutate(m["autonomy"])
        if rng.random() < 0.3:
            new["autonomy"] = mutate(new["autonomy"])
        c, _ = classify(m, new)
        if c in ("risk_reducing", "neutral"):
            for _ in range(40):
                a = {"purpose": rng.choice(["open", "increase"]), "session": rng.choice(["regular", "crypto"]),
                     "instrument": base.BTC, "asset_class": "crypto", "unusual_input": False,
                     "first_trade_in_instrument": False, "drawdown": "0", "daily_pnl_fraction": "0",
                     "position_usd_after": "0", "gross_usd_after": "0",
                     "new_instrument": rng.random() < 0.5, "thesis_confidence": rng.choice(["0", "0.5", "0.9"])}
                for f, vals in FIELDS_NUM.items():
                    a[f] = rng.choice(vals + [norm(D(v) + D("0.01")) for v in vals])
                d0, d1 = autonomy(m, a)["decision"], autonomy(new, a)["decision"]
                check(STRICT[d1] >= STRICT[d0], "MI-11 reducing change never loosens autonomy",
                      (m["autonomy"], new["autonomy"], a, d0, d1))
                if a["new_instrument"]:
                    check(STRICT[d0] >= STRICT[m["autonomy"]["admission"]],
                          "MI-17 an admission is never looser than the owner's admission ceiling", (m["autonomy"], a, d0))

INSTRUMENTS = [base.ABC, base.XYZ, base.QRS, base.LMN, base.BTC]
SRC_OK = ["src.filings", "src.newswire"]
TH_NOW = "2026-09-22T14:00:00.000000000Z"

def rand_research_mandate():
    m = copy.deepcopy(base.research)
    u = m["universe"]
    u["max_instruments"] = rng.randint(1, 4)
    u["asset_classes"] = rng.choice([["us_equity"], ["crypto", "us_equity"]])
    u["leveraged_etps_enabled"] = rng.random() < 0.6
    if u["leveraged_etps_enabled"]:
        u["leveraged_etp_disclosure_version"] = "sha256:" + "b" * 64
    m["protection"]["crypto_stop_limit_offset"] = "0.005" if "crypto" in u["asset_classes"] else None
    m["autonomy"]["admission"] = rng.choice(["ask", "deny", "auto"])
    m["behavior"]["research"]["max_revisions_per_lineage"] = rng.randint(0, 3)
    if rng.random() < 0.25:
        u.update({"pinned": True, "max_instruments": 1,
                  "pinned_instruments": [{"asset_id": base.XYZ, "symbol": "XYZ", "asset_class": "us_equity"}],
                  "asset_classes": ["us_equity"]})
        m["protection"]["crypto_stop_limit_offset"] = None
        m["behavior"]["research"] = None
        for sm in m["behavior"]["signal_models"]:
            sm["admits_instruments"] = False
    V.validate(m)
    assert semantic(m, dict(base.CTX, disclosures_accepted=["sha256:" + "b" * 64]))[0] == [], m["universe"]
    return m

def rand_thesis(i, lineage, revision):
    inst = rng.choice(INSTRUMENTS)
    horizon = rng.choice([3600, 86400, 604800])
    as_of = TH_NOW
    bad_expiry = rng.random() < 0.1
    return {"thesis_id": f"th-{i}", "lineage_id": lineage, "revision": revision,
            "predecessor_thesis_id": (f"th-{i - 1}" if revision > 0 else None) if rng.random() > 0.1 else None,
            "instrument_id": inst, "asset_class": "crypto" if inst == base.BTC else "us_equity",
            "direction": rng.choice(["long", "long", "long", "short"]), "horizon_s": horizon,
            "conviction": norm(D(rng.randint(0, 100)) / 100), "confidence": norm(D(rng.randint(0, 100)) / 100),
            "as_of": as_of,
            "expires_at": fmt(T(as_of) + timedelta(seconds=horizon + (60 if bad_expiry else 0))),
            "evidence_sources": rng.choice([SRC_OK, ["src.filings"], ["src.anonymous_blog"], []]),
            "corroboration": rng.choice([{"kind": "independent_source"}, {"kind": "market_data"}, {}]),
            "leveraged_etp": rng.random() < 0.45, "invalidation": "The trend breaks."}

def fuzz_admission(n):
    """MI-15, MI-16, MI-17, MI-18, MI-20: the working universe stays inside the envelope, whatever a thesis says."""
    for _ in range(n):
        m = rand_research_mandate()
        frozen = copy.deepcopy(m)
        u = m["universe"]
        allowed, cap = u["asset_classes"], u["max_instruments"]
        rev_cap = m["behavior"]["research"]["max_revisions_per_lineage"] if m["behavior"]["research"] else 0
        accepted = ["sha256:" + "b" * 64] if rng.random() < 0.5 else []
        etp_ok = u["leveraged_etps_enabled"] and u["leveraged_etp_disclosure_version"] in accepted
        elig = rng.sample(INSTRUMENTS, rng.randint(0, 2))
        halted = rng.sample(INSTRUMENTS, rng.randint(0, 1))
        groups = {base.QRS: "grp_q", base.LMN: "grp_q"}
        claimed = [base.LMN] if rng.random() < 0.3 else []
        universe, lineages = [], {}
        admitted_revisions = {}           # independent per-lineage counter (MI-18)
        lineage = "lin-0"
        for i in range(rng.randint(4, 14)):
            if rng.random() < 0.3:
                lineage = f"lin-{i}"
                revision = 0
            else:
                revision = admitted_revisions.get(lineage, 0) + (1 if rng.random() < 0.7 else 0)
            th = rand_thesis(i, lineage, revision)
            inp = {"thesis": th, "working_universe": universe, "eligibility_failures": elig,
                   "disclosures_accepted": accepted,
                   "allowlisted_sources": SRC_OK, "instrument_groups": groups, "claimed_by_other_agents": claimed,
                   "halted_instruments": halted, "data_universe": None,
                   "research_spend_usd_today": rng.choice(["0", "0", "5"]), "lineages": lineages,
                   "admission_action": {"order_usd": "300", "combined_score": "0.8", "instrument": th["instrument_id"],
                                        "asset_class": th["asset_class"], "session": "regular",
                                        "first_trade_in_instrument": True, "drawdown": "0", "daily_pnl_fraction": "0",
                                        "position_usd_after": "300", "gross_usd_after": "300",
                                        "bought_today_usd": "300", "position_pnl_fraction": "0",
                                        "unusual_input": False}}
            r = admit(m, inp)
            check(m == frozen, "MI-16 admitting a thesis never changes an envelope field", (th, m))
            check(len(r["working_universe"]) <= cap, "MI-15 the working universe never exceeds max_instruments",
                  (cap, r["working_universe"]))
            if u["pinned"]:
                check(not r["admitted"], "MI-20 a pinned universe admits nothing", (th, r))
            if r["admitted"] and r["change"] == "admitted":
                check(th["instrument_id"] not in universe, "MI-15 an admission never duplicates an entry", (th, r))
                check(th["asset_class"] in allowed, "MI-15 an admitted instrument is in an allowed asset class", (th, allowed))
                check(th["instrument_id"] not in elig, "MI-15 an admitted instrument passed the eligibility floor", (th, elig))
                check(th["instrument_id"] not in halted, "MI-15 an admitted instrument is not halted", (th, halted))
                check(bool(th["corroboration"].get("kind")), "MI-15 an admitted thesis is corroborated", th)
                check(all(s in SRC_OK for s in th["evidence_sources"]),
                      "MI-15 an admitted thesis cites only allowlisted sources", th)
                check(not (th["leveraged_etp"] and not etp_ok),
                      "MI-15 a leveraged ETP needs the owner opt-in and the accepted disclosure", th)
                check(th["direction"] == "long", "MI-15 v1 admits long theses only", th)
                check(T(th["expires_at"]) == T(th["as_of"]) + timedelta(seconds=th["horizon_s"]),
                      "MI-15 an admitted thesis expires at its horizon", th)
            if r["admitted"]:
                check(STRICT[r["first_order_autonomy"]["decision"]] >= STRICT[m["autonomy"]["admission"]],
                      "MI-17 the first order in an admitted instrument respects the admission ceiling", (th, r))
                check(th["revision"] <= rev_cap, "MI-18 no revision past the lineage cap is admitted", (th, rev_cap))
                admitted_revisions[th["lineage_id"]] = max(admitted_revisions.get(th["lineage_id"], 0), th["revision"])
                lineages.setdefault(th["lineage_id"], {"revisions": 0, "admitted": 0, "retired": False})
                lineages[th["lineage_id"]]["revisions"] = admitted_revisions[th["lineage_id"]]
            elif r["reason"] == "lineage_retired":
                lineages.setdefault(th["lineage_id"], {"revisions": 0, "admitted": 0, "retired": False})["retired"] = True
            universe = r["working_universe"]
            check(len(set(universe)) == len(universe), "MI-15 the working universe holds no duplicate", universe)
        check(all(v <= rev_cap for v in admitted_revisions.values()),
              "MI-18 a lineage never exceeds max_revisions_per_lineage", (admitted_revisions, rev_cap))

def fuzz_lineage(n):
    """MI-18, MI-19: a retired lineage admits nothing more and holds no instrument in the universe."""
    for _ in range(n):
        m = rand_research_mandate()
        if m["universe"]["pinned"]:
            continue
        cap = m["behavior"]["research"]["max_revisions_per_lineage"]
        # Two lineages over overlapping instruments, so one can take over what the other holds.
        theses, revs = [], {"lin-0": -1, "lin-1": -1}
        pool = [base.ABC, base.XYZ]
        for i in range(rng.randint(3, 10)):
            lin = rng.choice(["lin-0", "lin-1"])
            revs[lin] += 1
            th = rand_thesis(i, lin, revs[lin])
            th.update(direction="long", evidence_sources=SRC_OK, leveraged_etp=False,
                      instrument_id=rng.choice(pool), asset_class="us_equity",
                      corroboration={"kind": "independent_source"},
                      predecessor_thesis_id=f"{lin}-{revs[lin] - 1}" if revs[lin] > 0 else None,
                      expires_at=fmt(T(TH_NOW) + timedelta(seconds=th["horizon_s"])))
            if rng.random() < 0.3:
                # An over-cap revision that an earlier §8.5 check refuses must retire nothing, so the
                # fold has to follow the journaled reason rather than the revision number.
                th["direction"] = "short"
            theses.append(th)
        inp = {"theses": theses, "working_universe": [], "eligibility_failures": [],
               "allowlisted_sources": SRC_OK, "instrument_groups": {}, "claimed_by_other_agents": [],
               "halted_instruments": [], "data_universe": None, "research_spend_usd_today": "0",
               "disclosures_accepted": ["sha256:" + "b" * 64],
               "admission_action": {"order_usd": "300", "combined_score": "0.8", "instrument": base.ABC,
                                    "asset_class": "us_equity", "session": "regular",
                                    "first_trade_in_instrument": True, "drawdown": "0",
                                    "daily_pnl_fraction": "0", "position_usd_after": "300",
                                    "gross_usd_after": "300", "bought_today_usd": "300",
                                    "position_pnl_fraction": "0", "unusual_input": False}}
        r = lineage_fold(m, inp)
        # Independent holder model: the last lineage whose thesis for an instrument was admitted.
        holder, retired, admitted_rev = {}, set(), {}
        for th, s in zip(theses, r["steps"]):
            lin, inst = th["lineage_id"], th["instrument_id"]
            removed = [j for j in s["journal"] if j["type"] == "UniverseChanged" and j["change"] == "removed"]
            check(not removed or s["reason"] == "lineage_retired",
                  "MI-19 a refusal removes only when its journaled reason is lineage_retired", (s["reason"], removed))
            for j in removed:
                check(holder.get(j["instrument"]) == lin,
                      "MI-19 retirement removes only what the retiring lineage still holds",
                      (lin, j["instrument"], dict(holder)))
            if s["admitted"]:
                check(th["revision"] <= cap, "MI-18 no revision past the cap is admitted", (cap, th["revision"]))
                check(lin not in retired, "MI-18 a retired lineage admits nothing more", (lin, s["reason"]))
                holder[inst] = lin
                admitted_rev[lin] = max(admitted_rev.get(lin, 0), th["revision"])
            if s["reason"] == "lineage_retired":
                retired.add(lin)
                if holder.get(th["instrument_id"]) == lin:
                    holder.pop(th["instrument_id"], None)
        check(all(v <= cap for v in admitted_rev.values()),
              "MI-18 no lineage exceeds max_revisions_per_lineage", (cap, admitted_rev))
        for lin in retired:
            held = r["lineage_instruments"].get(lin)
            check(held is None or held not in r["working_universe"] or holder.get(held) not in (None, lin),
                  "MI-19 a retired lineage holds no instrument in the working universe",
                  (lin, held, r["working_universe"], dict(holder)))

def fuzz_expiry(n):
    """MI-19 and DEC-118: exactly the invalidated, retired, and expired entries are removed."""
    for _ in range(n):
        now = T(TH_NOW) + timedelta(seconds=rng.choice([0, 3600, 86400, 604800]))
        entries, lineages = [], {}
        for i, inst in enumerate(rng.sample(INSTRUMENTS, rng.randint(1, 5))):
            lin = f"lin-{i}"
            lineages[lin] = {"revisions": 0, "admitted": 1, "retired": rng.random() < 0.2}
            entries.append({"instrument": inst, "thesis_id": f"th-{i}", "lineage_id": lin, "revision": 0,
                            "expires_at": fmt(T(TH_NOW) + timedelta(seconds=rng.choice([3600, 86400, 604800]))),
                            "invalidated": rng.random() < 0.3})
        inp = {"now": fmt(now), "entries": entries, "lineages": lineages}
        r = thesis_expiry(base.research, inp)
        expected = sorted(e["instrument"] for e in entries
                          if e["invalidated"] or lineages[e["lineage_id"]]["retired"] or now >= T(e["expires_at"]))
        check(sorted(r["removed"]) == expected, "MI-19 exactly the ended theses are removed", (inp, r))
        check(sorted(r["working_universe"]) == sorted(e["instrument"] for e in entries
                                                     if e["instrument"] not in expected),
              "MI-19 every other entry stays", (inp, r))
        check(all(v == "removed_instrument" for v in r["instrument_restrictions"].values()),
              "MI-19 removal restricts only the instrument, never the agent", (inp, r))

PLACES = 12  # the size fraction the order builder multiplies targets by (§8.3 step 2)

def fits(x):
    return x == x.quantize(D(1).scaleb(-PLACES))

def ladder_with(factors):
    m = copy.deepcopy(base.swing)
    lad = [{"at": norm(D(i + 1) / 1000), "action": "scale_sizes", "factor": f} for i, f in enumerate(factors)]
    m["risk"]["drawdown_ladder"] = lad + [{"at": "0.06", "action": "exits_only", "factor": None},
                                          {"at": "0.08", "action": "flatten_and_pause", "factor": None}]
    return m

def cancelling_factor():
    """A factor whose significand is a power of 2 or of 5, so that products of such factors cancel
    places: 2^k x 5^k is a power of ten, and a whole ladder can need fewer places than one of its rungs."""
    v = (2 if rng.random() < 0.5 else 5) ** rng.randint(1, 15)
    return "0." + "0" * rng.randint(0, 3) + str(v)

def fuzz_ladder_precision(n):
    """V-040 (DEC-167): a valid ladder's size factor is exact at 12 places for every set of active rungs,
    and a ladder whose factors' places sum to at most 12 is never refused. The oracle multiplies every
    subset exactly; the pinned ladders fix both boundaries and a ladder whose whole product fits while a
    subset does not, which is why the rule is a sum and not a whole-product bound."""
    from itertools import combinations
    ctx = dict(base.CTX, disclosures_accepted=["sha256:" + "b" * 64])
    pinned = [(["0.123456789012"], False), (["0.1234567890123"], True),
              (["0.123456", "0.123456"], False), (["0.123456", "0.1234567"], True),
              (["0.0000000008192", "0.1220703125"], True)]
    for factors, refused in pinned:
        check(("V-040" in semantic(ladder_with(factors), ctx)[0]) == refused, "V-040: a pinned ladder", factors)
    check(fits(D("0.0000000008192") * D("0.1220703125")) and not fits(D("0.0000000008192")),
          "V-040: the pinned 2^13 x 5^13 ladder is the one where the whole product fits and a subset does not", None)
    for _ in range(n):
        k = rng.randint(1, 4)
        factors = [cancelling_factor() if rng.random() < 0.4 else
                   "0." + "".join(rng.choice("0123456789") for _ in range(rng.randint(0, 6))) + rng.choice("123456789")
                   for _ in range(k)]
        errs = semantic(ladder_with(factors), ctx)[0]
        every = all(fits(reduce_mul(sub)) for r in range(1, k + 1) for sub in combinations([D(f) for f in factors], r))
        places = sum(len(f) - 2 for f in factors)
        check("V-040" in errs or every, "V-040: a valid ladder's size factor needs more than 12 places", factors)
        check(places > PLACES or "V-040" not in errs, "V-040: a ladder whose factors fit is refused", factors)
        check(places <= PLACES or "V-040" in errs, "V-040: factors past 12 places are accepted", factors)

def reduce_mul(xs):
    out = D(1)
    for x in xs:
        out *= x
    return out

# ------------------------------------------------------------------ delegations (§6.5, MI-26 to MI-29, V-041 to V-043)
DELEG_NOW = T("2026-09-22T14:00:00.000000000Z")
SPAN_CAP_S = 30 * 24 * 3600
TROUBLE = [{"mode": "exits_only"}, {"mode": "paused"}, {"mode": "stopped"}, {"rungs_active": 1},
           {"limit_pending": True}, {"limit_latched": True}, {"kill_switch": True}]

def rand_autonomy_action():
    a = {"purpose": rng.choice(["open", "increase"]), "session": rng.choice(["regular", "crypto"]),
         "instrument": base.BTC, "asset_class": "crypto", "unusual_input": False,
         "first_trade_in_instrument": False, "drawdown": "0", "daily_pnl_fraction": "0",
         "position_usd_after": "0", "gross_usd_after": "0",
         "new_instrument": rng.random() < 0.2, "thesis_confidence": rng.choice(["0", "0.5", "0.9"])}
    for f, vals in FIELDS_NUM.items():
        a[f] = rng.choice(vals)
    a["order_usd"] = rng.choice(["100", "300", "500", "900", "1000"])
    return a

def rand_delegation(i, sources):
    start = DELEG_NOW + timedelta(seconds=rng.choice([-86400, -3600, 0, 3600]))
    span = rng.choice([60, 3600, 86400, 7 * 86400, SPAN_CAP_S])
    cap = rng.choice(["100", "500", "900", "1000"])
    when = rng.choice([{"field": "purpose", "op": "in", "value": sorted(rng.sample(["open", "increase"], rng.randint(1, 2)))},
                       {"field": "session", "op": "eq", "value": rng.choice(["regular", "crypto"])},
                       {"field": "order_usd", "op": "lte", "value": rng.choice(["300", "900"])}])
    return {"id": f"d{i}", "lifts": rng.choice(sorted(sources)), "when": when, "max_order_usd": cap,
            "max_orders": rng.randint(1, 4), "max_total_usd": norm(D(cap) * rng.choice([1, 2, 3, 10])),
            "starts_at": fmt(start), "expires_at": fmt(start + timedelta(seconds=span)), "source_approval_id": None}

def rand_delegated_mandate(any_source=False):
    """A mandate with delegations. With `any_source`, a delegation may name a source that is not an ask, which V-041
    refuses; the runtime must still never lift it (defence in depth for MI-26)."""
    m = copy.deepcopy(base.btc)
    au = m["autonomy"]
    au["rules"] = [rand_rule(i) for i in range(rng.randint(0, 4))]
    au["default"] = rng.choice(["ask", "ask", "deny", "auto"])
    au["admission"] = rng.choice(["ask", "deny", "auto"])
    every = {f"rule:{r['id']}" for r in au["rules"]} | {"default"}
    asks = {f"rule:{r['id']}" for r in au["rules"] if r["then"] == "ask"} | ({"default"} if au["default"] == "ask" else set())
    sources = every if any_source else asks
    if not sources:
        return None
    au["delegations"] = [rand_delegation(i, sources) for i in range(rng.randint(1, 3))]
    V.validate(m)
    return m

def rand_state(t, usage, trouble_p=0.3):
    st = {"now": fmt(t), "usage": usage}
    if rng.random() < trouble_p:
        st.update(rng.choice(TROUBLE))
    return st

def in_trouble(st):
    return any(st.get(k) for k in ("rungs_active", "limit_pending", "limit_latched", "kill_switch")) \
        or st.get("mode", "normal") != "normal"

def fuzz_delegations(n):
    """MI-26, MI-27, MI-28: a delegation only lifts the ask it names, within its caps and window, and never in trouble.
    The oracle keeps its own decision log and derives usage and the caps' totals from it."""
    for _ in range(n):
        m = rand_delegated_mandate(any_source=rng.random() < 0.3)
        if m is None:
            continue
        plain = copy.deepcopy(m)
        del plain["autonomy"]["delegations"]
        by_id = {d["id"]: d for d in m["autonomy"]["delegations"]}
        log = []
        t = DELEG_NOW - timedelta(hours=2)
        for _ in range(rng.randint(5, 40)):
            t += timedelta(seconds=rng.choice([1, 60, 1800, 3600, 86400, 5 * 86400]))
            usage = {}
            for did, _, v in log:
                u = usage.setdefault(did, {"orders": 0, "total_usd": "0"})
                u["orders"] += 1
                u["total_usd"] = norm(D(u["total_usd"]) + v)
            st = rand_state(t, usage)
            a = rand_autonomy_action()
            r0, r = autonomy(plain, a, st), autonomy(m, a, st)
            ctx = (m["autonomy"], a, st, r0, r)
            if r["decision"] != r0["decision"]:
                dl = by_id.get(r.get("delegation_id"))
                check(r0["decision"] == "ask" and r["decision"] == "auto" and dl is not None,
                      "MI-26 a delegation only turns an ask into auto", ctx)
                check(dl is not None and dl["lifts"] == r0["by"], "MI-26 a delegation lifts only the ask it names", ctx)
                check(dl is not None and cond(dl["when"], a), "MI-26 a delegation lifts only orders matching its condition", ctx)
            if a["new_instrument"]:
                check(STRICT[r["decision"]] >= STRICT[m["autonomy"]["admission"]],
                      "MI-26 no delegation makes an admission looser than the admission ceiling (MI-17)", ctx)
            if in_trouble(st):
                check(r["decision"] == r0["decision"], "MI-28 a delegation lifts nothing in trouble", ctx)
            if r.get("delegation_id") is not None and r["decision"] == "auto":
                log.append((r["delegation_id"], t, D(a["order_usd"])))
        for did, dl in by_id.items():
            mine = [(tt, v) for i, tt, v in log if i == did]
            ctx = (dl, mine)
            check(len(mine) <= dl["max_orders"], "MI-27 a delegation lifts at most max_orders", ctx)
            check(sum((v for _, v in mine), D(0)) <= D(dl["max_total_usd"]), "MI-27 a delegation lifts at most max_total_usd", ctx)
            check(all(v <= D(dl["max_order_usd"]) for _, v in mine), "MI-27 no lifted order exceeds max_order_usd", ctx)
            check(all(T(dl["starts_at"]) <= tt < T(dl["expires_at"]) for tt, _ in mine),
                  "MI-27 a delegation lifts only inside [starts_at, expires_at)", ctx)

def fuzz_client_ceiling(n):
    """MI-30: an order an owner-connected client requested is never auto, whatever the rules, default, or delegations
    say; a deny still denies, and an order the owner or the agent requested is decided exactly as before."""
    for _ in range(n):
        m = rand_delegated_mandate(any_source=rng.random() < 0.3)
        if m is None:
            continue
        m["autonomy"]["default"] = rng.choice(["auto", "ask", "deny"])
        if rng.random() < 0.3:
            m["autonomy"]["admission"] = "auto"
        plain = copy.deepcopy(m)
        plain["autonomy"]["delegations"] = []
        for _ in range(20):
            t = DELEG_NOW + timedelta(seconds=rng.randint(-86400, 35 * 86400))
            st = rand_state(t, {}, trouble_p=0.2)
            a = rand_autonomy_action()
            base_decision = autonomy(m, a, st)["decision"]
            without_delegations = autonomy(plain, a, st)["decision"]
            client = autonomy(m, dict(a, requested_by="client"), st)
            expected = "deny" if without_delegations == "deny" else "ask"
            ctx = (m["autonomy"], a, st, base_decision, client)
            check(client["decision"] != "auto", "MI-30 a client-requested order is never auto", ctx)
            check(client["decision"] == expected,
                  "MI-30 a client-requested order is ask, or deny when the rules deny, with no delegation lifting it", ctx)
            check(autonomy(m, dict(a, requested_by="owner"), st)["decision"] == base_decision,
                  "MI-30 the client ceiling leaves owner and agent requests unchanged", ctx)

def mutate_delegations(ds):
    """One random change to a delegation list; returns what kind of change it was."""
    k = rng.random()
    d = rng.choice(ds)
    later = lambda s, sec: fmt(T(s) + timedelta(seconds=sec))
    if k < 0.15:
        ds.remove(d)
    elif k < 0.3:
        c = rng.choice(DELEGATION_CAPS)
        d[c] = max(1, d[c] - 1) if c == "max_orders" else norm(D(d[c]) / 2)
    elif k < 0.45:
        d["expires_at"] = later(d["expires_at"], -rng.choice([30, 3600, 86400]))
    elif k < 0.55:
        d["starts_at"] = later(d["starts_at"], rng.choice([30, 3600]))
    elif k < 0.65:
        c = rng.choice(DELEGATION_CAPS)
        d[c] = d[c] + 2 if c == "max_orders" else norm(D(d[c]) * 2)
    elif k < 0.75:
        d["expires_at"] = later(d["expires_at"], rng.choice([3600, 86400, 3 * 86400]))
    elif k < 0.85:
        ds.append(dict(copy.deepcopy(d), id=f"d{len(ds) + 20}", starts_at=fmt(DELEG_NOW - timedelta(days=1)),
                       expires_at=fmt(DELEG_NOW + timedelta(days=10))))
    elif k < 0.92:
        d["when"] = {"field": "purpose", "op": "in", "value": ["increase", "open"]}
    elif len(ds) > 1:
        ds.reverse()

def fuzz_delegation_changes(n):
    """MI-29: a delegation change classified risk-reducing or neutral never makes a decision less strict."""
    for _ in range(n):
        m = rand_delegated_mandate()
        if m is None:
            continue
        new = copy.deepcopy(m)
        mutate_delegations(new["autonomy"]["delegations"])
        if rng.random() < 0.3 and new["autonomy"]["delegations"]:
            mutate_delegations(new["autonomy"]["delegations"])
        c, _ = classify(m, new)
        if c not in ("risk_reducing", "neutral"):
            continue
        ids = sorted({d["id"] for d in m["autonomy"]["delegations"] + new["autonomy"]["delegations"]})
        edges = [T(d[k]) for d in m["autonomy"]["delegations"] for k in ("starts_at", "expires_at")]
        for _ in range(30):
            if rng.random() < 0.5:
                t = rng.choice(edges) + timedelta(seconds=rng.choice([-1800, -1, 0, 1, 1800, 43200, 2 * 86400]))
            else:
                t = DELEG_NOW + timedelta(seconds=rng.randint(-2 * 86400, 35 * 86400))
            usage = {i: {"orders": rng.randint(0, 3), "total_usd": rng.choice(["0", "100", "900", "2000"])} for i in ids
                     if rng.random() < 0.5}
            st = rand_state(t, usage, trouble_p=0.1)
            a = rand_autonomy_action()
            d0, d1 = autonomy(m, a, st)["decision"], autonomy(new, a, st)["decision"]
            check(STRICT[d1] >= STRICT[d0], "MI-29 a reducing delegation change never decides less strictly",
                  (m["autonomy"]["delegations"], new["autonomy"]["delegations"], a, st, d0, d1))

def narrow_delegations(ds):
    """A delegation change the delegations row calls reducing: one removed, a cap lowered, or the window cut."""
    ds = copy.deepcopy(ds)
    k, d = rng.random(), rng.choice(ds)
    if k < 0.4:
        ds.remove(d)
    elif k < 0.7:
        c = rng.choice(DELEGATION_CAPS)
        d[c] = max(1, d[c] - 1) if c == "max_orders" else norm(D(d[c]) / 2)
    else:
        d["expires_at"] = fmt(max(T(d["starts_at"]) + timedelta(seconds=1), T(d["expires_at"]) - timedelta(seconds=rng.choice([30, 3600, 86400]))))
    return ds

def fuzz_delegated_rule_changes(n):
    """MI-29 across the two §9.2 rows (#444, DEC-353): a version classified reducing or neutral that changes the rules,
    with the delegations kept or narrowed in the same version, never makes any decision less strict than the previous
    version gave it, with each version's delegations in force and the same usage. An order `auto` by a rule may become
    `auto` by a delegation. The oracle compares decisions only, never the classifier's own conditions. Some versions
    name a source that is not an ask, which V-041 refuses. Some draws widen the rule a delegation names or make its
    source an ask, and some remove a rule ahead of it, on purpose.

    Two §9.2 bullets hold only because V-041 refuses a version (DEC-353 items 2 and 5): adding a rule a carried
    delegation already names, and a kept rule a delegation names going from `auto` to `ask`. Neither pair is valid,
    so the negative control asserts V-041 refuses its old version; a change to V-041 that admits it fails here."""
    for _ in range(n):
        m = rand_delegated_mandate(any_source=rng.random() < 0.3)
        if m is None:
            continue
        ds = m["autonomy"]["delegations"]
        if rng.random() < 0.1:
            bad = copy.deepcopy(m)
            d = rng.choice(bad["autonomy"]["delegations"])
            autos = [r["id"] for r in bad["autonomy"]["rules"] if r["then"] == "auto"]
            d["lifts"] = f"rule:{rng.choice(autos)}" if autos and rng.random() < 0.5 else "rule:not_in_this_version"
            check("V-041" in delegation_errors(bad, None),
                  "V-041 refuses a delegation naming a rule the version lacks or that is not an ask (§9.2 relies on it)",
                  (bad["autonomy"], d))
        new = copy.deepcopy(m)
        rest = copy.deepcopy({k: v for k, v in m["autonomy"].items() if k != "delegations"})
        named = rng.choice(ds)["lifts"]
        k = rng.random()
        if k < 0.45:
            for r in rest["rules"]:
                if f"rule:{r['id']}" == named:
                    w = r["when"]
                    wider = [v for v in FIELDS_NUM.get(w["field"], [])
                             if (D(v) < D(w["value"]) if w["op"] in ("gt", "gte") else D(v) > D(w["value"]))]
                    if wider and k < 0.35:
                        w["value"] = rng.choice(wider)
                    else:
                        r["then"] = "ask"
            if named == "default":
                rest["default"] = "ask"
        elif k < 0.65:
            ids = [f"rule:{r['id']}" for r in rest["rules"]]
            ahead = ids.index(named) if named in ids else len(ids)
            if ahead:
                rest["rules"].pop(rng.randrange(ahead))
        else:
            rest = mutate(rest)
        nds = narrow_delegations(ds) if rng.random() < 0.25 else copy.deepcopy(ds)
        new["autonomy"] = dict(rest, delegations=nds)
        c, _ = classify(m, new)
        if c not in ("risk_reducing", "neutral"):
            continue
        for _ in range(25):
            t = DELEG_NOW + timedelta(seconds=rng.randint(-3600, 3 * 86400))
            st = rand_state(t, {}, trouble_p=0.1)
            a = rand_autonomy_action()
            r0, r1 = autonomy(m, a, st), autonomy(new, a, st)
            check(STRICT[r1["decision"]] >= STRICT[r0["decision"]],
                  "MI-29 a reducing rule change never decides less strictly with the delegations in force",
                  (m["autonomy"], new["autonomy"], a, st, r0, r1))

def fuzz_delegation_rules(n):
    """V-041 (the 30-day span), V-042 (no carry-over past a risk-increasing version), V-043 (caps inside the envelope),
    each against a value the oracle computes itself."""
    for _ in range(n):
        m = copy.deepcopy(base.btc)
        two = rng.choice([None, "500", "1000"])
        m["autonomy"]["approval"]["two_approver_above_usd"] = two
        span = rng.choice([-10, 0, 1, 3600, SPAN_CAP_S - 1, SPAN_CAP_S, SPAN_CAP_S + 1, 40 * 86400])
        cap = rng.choice(["100", "500", "999", "1000", "1001"])
        total = rng.choice(["100", "1000", "10000", "10001"])
        d = {"id": "d1", "lifts": "rule:large_orders", "when": {"field": "purpose", "op": "in", "value": ["increase", "open"]},
             "max_order_usd": cap, "max_orders": 3, "max_total_usd": total, "starts_at": fmt(DELEG_NOW),
             "expires_at": fmt(DELEG_NOW + timedelta(seconds=span)), "source_approval_id": None}
        m["autonomy"]["delegations"] = [d]
        errs = semantic(m, base.CTX)[0]
        span_ok = 0 < span <= SPAN_CAP_S
        caps_ok = int(cap) <= 1000 and int(cap) <= int(total) <= 10000 and (two is None or int(cap) <= int(two))
        check(("V-041" in errs) != span_ok, "V-041 refuses exactly the spans outside (0, 30 days]", (span, errs))
        check(("V-043" in errs) != caps_ok, "V-043 refuses exactly the caps outside the envelope", (cap, total, two, errs))
        if not (span_ok and caps_ok):
            continue
        new = copy.deepcopy(m)
        increasing = rng.random() < 0.5
        new["risk"]["max_daily_loss"] = "0.03" if increasing else "0.01"
        if rng.random() < 0.5:
            new["autonomy"]["delegations"] = [dict(d, id="d2")]
        carried = new["autonomy"]["delegations"][0]["id"] == "d1"
        errs = semantic(new, dict(base.CTX, previous_version=m))[0]
        check(("V-042" in errs) == (increasing and carried),
              "V-042 refuses exactly a carried delegation in a version risk-increasing elsewhere", (increasing, carried, errs))
# ------------------------------------------------------------------ the review date (§6.2 step 5b, MI-32, V-046; DEC-188)
# The oracles below find the instant the review date passes by building 00:00 America/New_York on the next calendar
# day with zoneinfo, never through `risk_day`, and bound V-046 with date ordinals, never through `days_after`.
REVIEW_TZ = ZoneInfo("America/New_York")
REVIEW_DATES = ["2026-09-19", "2026-09-21", "2026-09-22", "2026-09-23", "2026-09-25", "2026-10-31", "2027-03-13"]
REVIEW_OFFSETS_S = [-5 * 3600, -4 * 3600, -3600, -1, 0, 1, 3600, 4 * 3600, 5 * 3600]

def own_review_end(rb):
    """The first instant past `rb`: 00:00 America/New_York on the following calendar day."""
    nd = date.fromordinal(date.fromisoformat(rb).toordinal() + 1)
    return datetime(nd.year, nd.month, nd.day, tzinfo=REVIEW_TZ).astimezone(timezone.utc)

def own_passed(rb, t):
    return rb is not None and t >= own_review_end(rb)

def review_times(rbs):
    """Risk-clock instants on both sides of each review date's end, a few hours either way so that a UTC-day reading
    and a New York-day reading disagree, plus instants spread over the delegation windows."""
    out = [own_review_end(rb) + timedelta(seconds=rng.choice(REVIEW_OFFSETS_S)) for rb in rbs for _ in range(3)]
    out += [DELEG_NOW + timedelta(seconds=rng.randint(-4 * 86400, 6 * 86400)) for _ in range(6)]
    return out

def rand_reviewed_mandate():
    """A mandate with random rules, default, admission and, often, delegations, and a review date."""
    m = rand_delegated_mandate(any_source=rng.random() < 0.3) if rng.random() < 0.6 else None
    if m is None:
        m = copy.deepcopy(base.btc)
        m["autonomy"]["rules"] = [rand_rule(i) for i in range(rng.randint(0, 4))]
        m["autonomy"]["default"] = rng.choice(["ask", "deny", "auto", "auto"])
        m["autonomy"]["admission"] = rng.choice(["ask", "deny", "auto"])
    m["autonomy"]["review_by"] = rng.choice(REVIEW_DATES)
    V.validate(m)
    return m

def without_review(m):
    out = copy.deepcopy(m)
    out["autonomy"].pop("review_by", None)
    return out

def rand_review_action():
    a = rand_autonomy_action()
    if rng.random() < 0.2:
        return {"purpose": rng.choice(sorted(REDUCING))}
    if rng.random() < 0.3:
        a["requested_by"] = rng.choice(["agent", "owner", "client"])
    return a

def fuzz_review(n):
    """MI-32: from 00:00 America/New_York after the review date, no auto survives for an opening or an increase,
    whatever the rules, the default, a delegation, or the admission setting say; a deny still denies and an ask keeps
    its own source; exits stay built-in AUTO; before the date every decision, label, and delegation id is what it is
    with no review date. Versions apply at random instants, so a re-confirmation racing the date is judged by the
    version in effect at each decision, which the oracle folds from its own application log."""
    for _ in range(n):
        m = rand_reviewed_mandate()
        versions = [(DELEG_NOW - timedelta(days=30), m)]
        if rng.random() < 0.4:
            later = copy.deepcopy(m)
            later["autonomy"]["review_by"] = rng.choice([d for d in REVIEW_DATES if d > m["autonomy"]["review_by"]] or ["2027-06-01"])
            versions.append((own_review_end(m["autonomy"]["review_by"]) + timedelta(seconds=rng.choice(REVIEW_OFFSETS_S)), later))
        rbs = [v["autonomy"]["review_by"] for _, v in versions]
        for t in sorted(review_times(rbs)):
            in_effect = [v for at_, v in versions if at_ <= t][-1]
            rb = in_effect["autonomy"]["review_by"]
            st = rand_state(t, {}, trouble_p=0.2)
            a = rand_review_action()
            r, r0 = autonomy(in_effect, a, st), autonomy(without_review(in_effect), a, st)
            ctx = (in_effect["autonomy"], a, st, r0, r)
            if a["purpose"] in REDUCING:
                check(r == {"decision": "auto", "by": "builtin_risk_reducing"}, "MI-32 an exit stays built-in AUTO past the review date", ctx)
            elif own_passed(rb, t):
                check(r["decision"] != "auto", "MI-32 no auto survives past the review date", ctx)
                check(r["decision"] == ("deny" if r0["decision"] == "deny" else "ask"),
                      "MI-32 past the review date a deny still denies and everything else asks", ctx)
                check(r0["decision"] != "ask" or r == r0, "MI-32 an ask past the review date keeps its own source", ctx)
                check(r0["decision"] != "auto" or r["by"] == "review_ceiling", "MI-32 the review ceiling names itself", ctx)
            else:
                check(r == r0, "MI-32 before the review date nothing changes", ctx)
            if a["purpose"] not in REDUCING:
                check(autonomy(in_effect, a)["decision"] != "auto", "MI-32 with no risk clock to judge by, nothing is auto", ctx)

def own_review_class(old_rb, new_rb):
    if old_rb == new_rb:
        return None
    if new_rb is None or (old_rb is not None and new_rb > old_rb):
        return "increasing"
    return "reducing"

def fuzz_review_changes(n):
    """§9.2 and MI-11, MI-29 for the review date: moving it earlier or setting it is reducing, moving it later or
    removing it is increasing, and a version classified reducing or neutral never decides less strictly at any instant."""
    choices = [None] + REVIEW_DATES
    for _ in range(n):
        old = rand_reviewed_mandate()
        old["autonomy"]["review_by"] = rng.choice(choices)
        if old["autonomy"]["review_by"] is None:
            del old["autonomy"]["review_by"]
        new = copy.deepcopy(old)
        new_rb = rng.choice(choices)
        new["autonomy"].pop("review_by", None)
        if new_rb is not None:
            new["autonomy"]["review_by"] = new_rb
        other = rng.random() < 0.4
        if other:
            kept = {k: new["autonomy"][k] for k in ("review_by", "delegations") if k in new["autonomy"]}
            new["autonomy"] = dict(mutate({k: v for k, v in new["autonomy"].items() if k not in kept}), **kept)
        old_rb = old["autonomy"].get("review_by")
        c, paths = classify(old, new)
        own = own_review_class(old_rb, new_rb)
        if not other and own is not None:
            check(c == {"increasing": "risk_increasing", "reducing": "risk_reducing"}[own],
                  "§9.2 the review date classifies as its own row", (old_rb, new_rb, c))
        if own == "increasing":
            check(c == "risk_increasing", "§9.2 a later or removed review date is risk-increasing", (old_rb, new_rb, c, paths))
        if c not in ("risk_reducing", "neutral"):
            continue
        for t in review_times([x for x in (old_rb, new_rb) if x is not None]):
            st = rand_state(t, {}, trouble_p=0.1)
            a = rand_review_action()
            d0, d1 = autonomy(old, a, st)["decision"], autonomy(new, a, st)["decision"]
            check(STRICT[d1] >= STRICT[d0], "MI-11 a reducing review-date change never loosens autonomy",
                  (old["autonomy"], new["autonomy"], a, st, d0, d1))

def fuzz_review_rules(n):
    """V-046 and V-020 for the review date, and V-042's carry on re-confirmation, against bounds the oracle computes
    from date ordinals: a date set or moved lies in [validation date, validation date + 180 days]; one carried unchanged
    is never re-checked; a set date is never removed; a platform default is exactly the validation date + 90 days; and
    re-confirming carries the delegations over unless the version is risk-increasing elsewhere."""
    for _ in range(n):
        vd = date(2026, 9, 24) + timedelta(days=rng.randint(-200, 200))
        prev_rb = rng.choice([None, None, (vd + timedelta(days=rng.randint(-120, 180))).isoformat()])
        offset = rng.choice([-400, -1, 0, 1, 89, 90, 91, 179, 180, 181, 400, rng.randint(-200, 200)])
        rb = rng.choice([None, prev_rb, (vd + timedelta(days=offset)).isoformat()])
        m = copy.deepcopy(base.btc)
        if rb is not None:
            m["autonomy"]["review_by"] = rb
        ctx = dict(base.CTX, validation_date=vd.isoformat())
        if prev_rb is not None or rng.random() < 0.5:
            ctx["previous_version"] = {"environment": m["environment"], "connection_id": m["connection_id"],
                                       "autonomy": {} if prev_rb is None else {"review_by": prev_rb}}
        source = rng.choice(["user_entered", "platform_default", "platform_proposed"])
        if rb is not None:
            ctx["provenance"] = {"/autonomy/review_by": {"source": source, "confirmed": True}}
        errs = semantic(m, ctx)[0]
        prev = prev_rb if "previous_version" in ctx else None
        span = None if rb is None else date.fromisoformat(rb).toordinal() - vd.toordinal()
        bad = (prev is not None and rb is None) or (rb is not None and rb != prev and not 0 <= span <= 180)
        check(("V-046" in errs) == bad, "V-046 refuses exactly a removed date or a new one outside 180 days of validation",
              (vd, prev, rb, errs))
        default_ok = rb is None or source != "platform_default" or span == 90
        check(("V-020" in errs) != default_ok, "V-020 accepts a platform-default review date only at 90 days", (vd, rb, source, errs))
        d = copy.deepcopy(base.btc)
        d["autonomy"]["review_by"] = "2026-09-20"
        d["autonomy"]["delegations"] = [{"id": "d1", "lifts": "rule:large_orders", "when": {"field": "purpose", "op": "in", "value": ["increase", "open"]},
                                         "max_order_usd": "500", "max_orders": 2, "max_total_usd": "1000",
                                         "starts_at": fmt(DELEG_NOW), "expires_at": fmt(DELEG_NOW + timedelta(days=10)), "source_approval_id": None}]
        renewed = copy.deepcopy(d)
        renewed["autonomy"]["review_by"] = rng.choice(["2026-10-01", "2026-12-23", "2027-03-23"])
        elsewhere = rng.choice([None, "0.03", "0.01"])
        if elsewhere is not None:
            renewed["risk"]["max_daily_loss"] = elsewhere
        errs = semantic(renewed, dict(base.CTX, previous_version=d))[0]
        check(("V-042" in errs) == (elsewhere == "0.03"),
              "V-042 re-confirming carries delegations over unless the version is risk-increasing elsewhere", (elsewhere, errs))
        check("V-046" not in errs, "V-046 a re-confirmation inside 180 days is valid", errs)

# ------------------------------------------------------------------ tripwires (§6.7; MI-31, V-044; DEC-187, DEC-350 to DEC-352)
# The oracle keeps the raw input log and, after every input, recounts each tripwire's metric from scratch over its
# window with its own Fraction accounting and half-even rounding, its own arming log, and its own step-up ledger; it
# never calls the fold, its counters, fill_net_realized, or owner_command.
TW_INSTS = [base.ABC, base.XYZ, base.QRS, base.BTC]
TW_IDS = ["a_trip", "b_trip", "c_trip"]
TW_METRICS = ["consecutive_losing_exits", "new_instruments", "realized_loss_usd"]
TW_RANK = {"end_delegations": 0, "exits_only": 1}
TW_GATE_ST = {"now": "2026-09-22T15:00:00.000000000Z", "agent_equity": "10000", "positions_mv": {}, "working_opening_orders": [],
              "orders_today": 0, "last_exit_fill_at": {}, "working_universe": [base.ABC]}

def own_round12(x):
    n, rem = divmod(x * 10 ** 12, 1)
    n = int(n)
    if rem > Fraction(1, 2) or (rem == Fraction(1, 2) and n % 2):
        n += 1
    return Fraction(n, 10 ** 12)

def own_nets(log):
    """Net realized per fill index, over the whole log: a buy's is minus its fees; a sell's is its proceeds less the
    basis it removes (all of it when it closes) less its fees."""
    qty, basis, nets = {}, {}, {}
    for i, e in enumerate(log):
        if e["event"] not in ("FillApplied", "LateFillApplied"):
            continue
        k, q, p, fee = e["instrument"], Fraction(e["qty"]), Fraction(e["price"]), Fraction(e["fees"])
        Q, B = qty.get(k, Fraction(0)), basis.get(k, Fraction(0))
        if e["side"] == "buy":
            qty[k], basis[k], g = Q + q, B + q * p, Fraction(0)
        elif q == Q:
            qty[k], basis[k], g = Fraction(0), Fraction(0), q * p - B
        else:
            removed = min(B, own_round12(B * q / Q))
            qty[k], basis[k], g = Q - q, B - removed, q * p - removed
        nets[i] = g - fee
    return nets

def own_metric(metric, window, log, nets, day):
    fills = [j for j in window if j in nets]
    if metric == "consecutive_losing_exits":
        n = 0
        for j in reversed(fills):
            if log[j]["side"] == "sell":
                if nets[j] >= 0:
                    break
                n += 1
        return Fraction(n)
    if metric == "new_instruments":
        return Fraction(sum(1 for j in fills if all(log[i]["instrument"] != log[j]["instrument"] for i in nets if i < j)))
    return max(Fraction(0), -sum((nets[j] for j in fills if j > day), Fraction(0)))

def own_tripwires(log, env):
    """Per input: the fired ids with the action each holds, the ids newly fired and lifted, refused acknowledgments,
    and every armed tripwire's metric, recounted from scratch."""
    nets = own_nets(log)
    fired, arm, seen, cur, out = {}, {}, set(), {}, []
    for k, e in enumerate(log):
        lifted, refused = [], False
        if e["event"] == "MandateVersionApplied":
            new = {t["id"]: t for t in e["mandate"]["autonomy"].get("tripwires", [])}
            for i, t in new.items():
                if i not in cur or cur[i]["metric"] != t["metric"]:
                    arm[i] = k
            cur = new
        elif e["event"] == "OwnerAcknowledged":
            ok = own_step_up_ok(e["own_ev"], e["at_s"], env, seen)
            if isinstance(e["own_ev"], dict) and isinstance(e["own_ev"].get("assertion"), str):
                seen.add(e["own_ev"]["assertion"])
            if e.get("independent_approval_required") or e.get("independent_now"):
                ok = ok and None not in (e.get("user"), e.get("requester")) and e["user"] != e["requester"]
            refused = not ok
            if ok and e["tripwire"] in fired:
                del fired[e["tripwire"]]
                lifted.append(e["tripwire"])
                if e["tripwire"] in cur:
                    arm[e["tripwire"]] = k
        day = max([j for j in range(k + 1) if log[j]["event"] == "RiskDayStarted"], default=-1)
        metrics = {i: own_metric(t["metric"], range(arm[i] + 1, k + 1), log, nets, day) for i, t in cur.items()}
        newly = [i for i in sorted(cur) if i not in fired and metrics[i] >= Fraction(cur[i]["threshold"])]
        for i in newly:
            fired[i] = cur[i]["action"]
        held = {i: (cur[i]["action"] if i in cur and TW_RANK[cur[i]["action"]] > TW_RANK[a] else a) for i, a in fired.items()}
        out.append({"held": held, "newly": newly, "lifted": lifted, "refused": refused, "metrics": metrics})
    return out

def rand_tripwire(i):
    metric = rng.choice(TW_METRICS)
    th = str(rng.randint(1, 4)) if metric != "realized_loss_usd" else rng.choice(["0.01", "1", "5", "20", "50.5", "200"])
    return {"id": i, "metric": metric, "threshold": th, "action": rng.choice(["end_delegations", "exits_only"])}

def rand_tripwires():
    return [rand_tripwire(i) for i in sorted(rng.sample(TW_IDS, rng.randint(0, 3)))]

def mutate_tripwires(tws):
    out = copy.deepcopy(tws)
    for _ in range(rng.randint(1, 2)):
        op = rng.choice(["add", "remove", "lower", "raise", "stricter", "softer", "metric"])
        free = [i for i in TW_IDS if i not in {t["id"] for t in out}]
        if op == "add" and free:
            out.append(rand_tripwire(rng.choice(free)))
        elif out:
            t = rng.choice(out)
            if op == "remove":
                out.remove(t)
            elif op in ("lower", "raise"):
                v = Fraction(t["threshold"])
                step = 1 if t["metric"] != "realized_loss_usd" else Fraction(rng.choice([1, 50, 500]), 100)
                v = v - step if op == "lower" else v + step
                if v > 0:
                    t["threshold"] = norm(D(v.numerator) / D(v.denominator))
            elif op == "stricter":
                t["action"] = "exits_only"
            elif op == "softer":
                t["action"] = "end_delegations"
            else:
                t["metric"] = rng.choice(TW_METRICS)
                if t["metric"] != "realized_loss_usd" and "." in t["threshold"]:
                    t["threshold"] = "2"
    return sorted(out, key=lambda t: t["id"])

def tw_mandate(tws):
    m = copy.deepcopy(base.BASES["research_equity"])
    if tws or rng.random() < 0.5:
        m["autonomy"]["tripwires"] = tws
    V.validate(m)
    return m

def rand_tw_history(n, t0, versions=True, acks=True):
    """A random account-stream history: fills that never sell more than is held, risk days, versions, and
    acknowledgments with good and bad step-up evidence. Evidence carries the oracle's integer `at_s`."""
    held, log, t = {}, [], t0
    used = []
    log.append({"event": "MandateVersionApplied", "at": ts(t), "at_s": t, "mandate": tw_mandate(rand_tripwires())})
    for _ in range(n):
        t += rng.choice([1, 5, 60, 600])
        r = rng.random()
        if r < 0.62:
            inst = rng.choice(TW_INSTS[:3])
            q = held.get(inst, Fraction(0))
            side = "sell" if q > 0 and rng.random() < 0.55 else "buy"
            if side == "buy":
                qty = Fraction(rng.choice([1, 2, 3, 5]))
            else:
                qty = rng.choice([q] + [x for x in (Fraction(1, 2), Fraction(1), Fraction(2)) if x < q])
            held[inst] = q + qty if side == "buy" else q - qty
            px = rng.choice(["99", "99.99", "100", "100.01", "101", "10.000000000001", "33.33"])
            log.append({"event": rng.choice(["FillApplied", "FillApplied", "LateFillApplied"]), "at": ts(t), "at_s": t,
                        "instrument": inst, "side": side, "qty": norm(D(qty.numerator) / D(qty.denominator)), "price": px,
                        "fees": rng.choice(["0", "0", "0.01", "1"])})
        elif r < 0.70:
            log.append({"event": "RiskDayStarted", "at": ts(t), "at_s": t})
        elif r < 0.82 and versions:
            prev = next(e["mandate"] for e in reversed(log) if e["event"] == "MandateVersionApplied")
            log.append({"event": "MandateVersionApplied", "at": ts(t), "at_s": t,
                        "mandate": tw_mandate(mutate_tripwires(prev["autonomy"].get("tripwires", [])))})
        elif acks:
            kind = rng.choice(["fresh", "fresh", "fresh", "stale", "future", "reused", "method", "none", "junk"])
            aid = f"as-{len(log)}-{rng.randint(0, 10 ** 6)}"
            ev = {"assertion": aid, "at_s": t - rng.randint(0, 300), "method": "cli_confirm"}
            if kind == "stale":
                ev["at_s"] = t - 301
            elif kind == "future":
                ev["at_s"] = t + 1
            elif kind == "reused" and used:
                ev["assertion"] = rng.choice(used)
            elif kind == "method":
                ev["method"] = "password"
            elif kind == "none":
                ev = None
            elif kind == "junk":
                ev = {"assertion": 7}
            if isinstance(ev, dict) and isinstance(ev.get("assertion"), str):
                used.append(ev["assertion"])
            ack = {"event": "OwnerAcknowledged", "at": ts(t), "at_s": t, "tripwire": rng.choice(TW_IDS),
                   "step_up": wire(ev), "own_ev": ev}
            if rng.random() < 0.4:
                ack.update(independent_approval_required=rng.random() < 0.6, independent_now=rng.random() < 0.6,
                           user=rng.choice(["user:u1", "user:u2", None]), requester=rng.choice(["user:u1", "user:u2", None]))
            log.append(ack)
    return log

def fuzz_tripwires(n):
    """MI-31 and MI-28: a tripwire fires exactly at the first input whose recounted metric reaches its threshold, stays
    fired through versions, risk days, and refused acknowledgments, and lifts only on an acknowledgment with valid
    step-up, after which it counts afresh; it alerts with opaque text; while fired no delegation lifts, an exits_only
    one holds openings and never exits, and nothing else changes; refolding a prefix and continuing gives the same."""
    for _ in range(n):
        log = rand_tw_history(rng.randint(5, 45), ESC_T0)
        steps = [{k: v for k, v in e.items() if k not in ("own_ev", "at_s")} for e in log]
        try:
            got = tripwire_run(steps)
        except AssertionError as ex:
            check(False, "MI-31 the fold accepts every history the oracle does", (str(ex), steps))
            continue
        own = own_tripwires(log, "paper")
        dm = rand_delegated_mandate()
        for k, (g, o) in enumerate(zip(got, own)):
            ctx = (k, steps[: k + 1], g, o)
            st = g["state"]
            check(st["fired"] == o["held"], "MI-31 the fired set and each held action match the recount", ctx)
            check({i: Fraction(v) for i, v in st["metrics"].items()} == o["metrics"], "MI-31 each armed metric matches the recount", ctx)
            trig = [j["limit"] for j in g["journal"] if j["type"] == "RiskLimitTriggered"]
            check(trig == [f"tripwire:{i}" for i in o["newly"]], "MI-31 a tripwire fires exactly at the first input reaching its threshold", ctx)
            lift = [j["limit"] for j in g["journal"] if j["type"] == "RiskLimitLifted"]
            check(lift == [f"tripwire:{i}" for i in o["lifted"]], "MI-31 only an acknowledgment with valid step-up lifts, and only a fired tripwire", ctx)
            check(any(j["type"] == "OwnerCommandRefused" for j in g["journal"]) == o["refused"],
                  "§6.1 an acknowledgment without valid step-up is refused and journaled", ctx)
            alerts = [j for j in g["journal"] if j["type"] == "OwnerAlertSent"]
            check(len(alerts) == len(o["newly"]), "MI-31 every firing alerts the owner once", ctx)
            check(all(set(a) == {"type", "subject", "text"} and a["text"] == TRIPWIRE_ALERT_TEXT
                      and not any(i in a["subject"] for i in TW_IDS + TW_INSTS) for a in alerts),
                  "rule 6 a tripwire alert carries an opaque subject and generic text only", ctx)
            check(st["restriction"] == ("exits_only" if "exits_only" in o["held"].values() else None),
                  "MI-31 an exits_only tripwire, and only one, restricts the agent to exits_only, never paused", ctx)
            check(st["delegations_suspended"] == bool(o["held"]), "MI-28 a fired tripwire, and only one, suspends delegations", ctx)
            if dm is not None:
                now = fmt(DELEG_NOW + timedelta(seconds=rng.randint(0, 3 * 86400)))
                a = rand_autonomy_action() if rng.random() < 0.8 else {"purpose": rng.choice(sorted(REDUCING))}
                calm = {"now": now, "usage": {}}
                r = autonomy(dm, a, tripwire_autonomy_state(st, now))
                plain = copy.deepcopy(dm)
                del plain["autonomy"]["delegations"]
                want = autonomy(plain if o["held"] else dm, a, calm)
                check(r == want, "MI-31 a fired tripwire changes a decision only by stopping every delegation", (ctx, a, r, want))
                mode = tripwire_autonomy_state(st, now)["mode"]
                for purpose in ("risk_exit", "protective", "owner_exit", "discretionary_exit", "open"):
                    v = order_decision(base.BASES["research_equity"], TW_GATE_ST,
                                       {"instrument": base.ABC, "purpose": purpose, "qty": "1", "limit_price": "100"},
                                       mode, set(), "regular", False, owner_confirmed_bid=True)
                    if purpose == "open":
                        check((v["verdict"] == "deny") == ("exits_only" in o["held"].values()),
                              "MI-31 an exits_only tripwire holds openings, and nothing else does", (ctx, v))
                    else:
                        check(v["verdict"] == "allow", "MI-1 a tripwire never holds or denies an exit", (ctx, purpose, v))
        cut = rng.randint(0, len(steps))
        tw = Tripwires()
        for s in steps[:cut]:
            tw.step(copy.deepcopy(s))
        rest = [{"journal": tw.step(s), "state": tw.snapshot()} for s in steps[cut:]]
        check(rest == got[cut:], "MI-8 refolding a prefix and continuing gives the same journal and state", cut)

def fuzz_tripwire_latch(n):
    """MI-7 and §5.9 through the risk state itself: while the recount says a tripwire is fired, the risk state holds it
    as a latched limit and rejects every allocation increase, and it has restriction `tripwire` exactly while one
    holds `exits_only`. The risk state's one instrument starts held, so the oracle's log opens with that buy."""
    for _ in range(n):
        m = copy.deepcopy(base.swing)
        m["risk"].update({"max_position_usd": "10000", "max_position_fraction": "1", "max_gross_exposure_usd": "10000"})
        tws = [t for t in rand_tripwires() if t["metric"] != "new_instruments"] or [rand_tripwire("a_trip") | {"metric": "consecutive_losing_exits", "threshold": "2"}]
        m["autonomy"]["tripwires"] = tws
        V.validate(m)
        t = ESC_T0
        rs = RiskState(m, "20", "100", "us_equity", ts(t))
        log = [{"event": "FillApplied", "instrument": TW_AGENT_INSTRUMENT, "side": "buy", "qty": "20", "price": "100", "fees": "0"},
               {"event": "MandateVersionApplied", "mandate": m}]
        held = Fraction(20)
        for k in range(rng.randint(5, 30)):
            t += rng.choice([1, 5, 30])
            r = rng.random()
            if r < 0.55:
                side = "sell" if held > 1 and rng.random() < 0.7 else "buy"
                qty = Fraction(rng.choice([1, 2])) if side == "buy" else Fraction(1)
                held += qty if side == "buy" else -qty
                px = rng.choice(["98", "99.5", "100", "101", "130"])
                step = {"event": "fill", "at": ts(t), "side": side, "qty": str(qty), "price": px, "session": "regular"}
                log.append({"event": "FillApplied", "instrument": TW_AGENT_INSTRUMENT, "side": side, "qty": str(qty),
                            "price": px, "fees": "0"})
            elif r < 0.62:
                step = {"event": "risk_day_started", "at": ts(t), "session": "regular"}
                log.append({"event": "RiskDayStarted"})
            elif r < 0.8:
                ev = {"assertion": f"as-{k}-{rng.randint(0, 10 ** 6)}", "at_s": t - rng.choice([0, 60, 301]), "method": "cli_confirm"}
                tid = rng.choice([x["id"] for x in tws])
                step = {"event": "owner_acknowledged", "at": ts(t), "restriction": f"tripwire:{tid}", "session": "regular",
                        "step_up": wire(ev)}
                own = {"event": "OwnerAcknowledged", "tripwire": tid, "own_ev": ev, "at_s": t}
                if rng.random() < 0.3:
                    extra = {"independent_approval_required": rng.random() < 0.6, "independent_now": rng.random() < 0.6,
                             "user": rng.choice(["user:u1", "user:u2", None]), "requester": rng.choice(["user:u1", None])}
                    step.update(extra)
                    own.update(extra)
                log.append(own)
            else:
                step = {"event": "allocation_change", "at": ts(t), "delta_usd": rng.choice(["100", "-100"]), "session": "regular"}
            before = own_tripwires(log, "paper")[-1]["held"]
            out = rs.step(step)
            after = own_tripwires(log, "paper")[-1]["held"]
            ctx = (tws, step, before, after, out.get("error"), out["restrictions"])
            if step["event"] == "allocation_change" and step["delta_usd"] == "100" and before:
                check(out.get("error") == "increase_blocked_while_latched", "MI-7 a fired tripwire rejects an allocation increase", ctx)
            check(("tripwire" in out["restrictions"]) == ("exits_only" in after.values()),
                  "§5.9 restriction tripwire exactly while a fired tripwire holds exits_only", ctx)
            check({k[len("tripwire:"):] for k in rs.latched if k.startswith("tripwire:")} == set(after),
                  "§5.8 every fired tripwire, and only one, is a latched limit", ctx)

def own_tripwire_class(ot, nt):
    """§9.2 for tripwires, by id: None when equal; increasing if one is gone, changed its metric, rose, or softened."""
    if ot == nt:
        return None
    by = {t["id"]: t for t in nt}
    worse = any(t["id"] not in by or by[t["id"]]["metric"] != t["metric"]
                or Fraction(by[t["id"]]["threshold"]) > Fraction(t["threshold"])
                or (t["action"], by[t["id"]]["action"]) == ("exits_only", "end_delegations") for t in ot)
    return "increasing" if worse else "reducing"

def fuzz_tripwire_changes(n):
    """§9.2 for tripwires, and MI-31's version clause with MI-11: a version classified reducing or neutral, applied at a
    random input of a random history without acknowledgments, never lifts a fired tripwire, never fires one later or
    with a softer action, and never decides less strictly than the previous version would have."""
    for _ in range(n):
        old_t = rand_tripwires()
        new_t = mutate_tripwires(old_t)
        old, new = tw_mandate(old_t), tw_mandate(new_t)
        c, paths = classify(old, new)
        own = own_tripwire_class(old_t, new_t)
        want = {None: "neutral", "increasing": "risk_increasing", "reducing": "risk_reducing"}[own]
        check(c == want, "§9.2 tripwires classify by their own row", (old_t, new_t, c, paths))
        if c == "risk_increasing":
            continue
        log = rand_tw_history(rng.randint(5, 40), ESC_T0, versions=False, acks=False)
        log[0]["mandate"] = old
        at = rng.randint(1, len(log))
        t = log[at - 1]["at_s"]
        a_log = log[:at] + [{"event": "MandateVersionApplied", "at": ts(t), "at_s": t, "mandate": old}] + log[at:]
        b_log = log[:at] + [{"event": "MandateVersionApplied", "at": ts(t), "at_s": t, "mandate": new}] + log[at:]
        strip = lambda lg: [{k: v for k, v in e.items() if k not in ("own_ev", "at_s")} for e in lg]
        ra, rb = tripwire_run(strip(a_log)), tripwire_run(strip(b_log))
        first_a, first_b = {}, {}
        for k, (x, y) in enumerate(zip(ra, rb)):
            fa, fb = x["state"]["fired"], y["state"]["fired"]
            ctx = (old_t, new_t, at, k, fa, fb)
            check(all(i in fb and TW_RANK[fb[i]] >= TW_RANK[fa[i]] for i in fa),
                  "MI-31 a reducing version never lifts a fired tripwire or softens what it holds", ctx)
            for i in fa:
                first_a.setdefault(i, k)
            for i in fb:
                first_b.setdefault(i, k)
            now = fmt(DELEG_NOW)
            dm = rand_delegated_mandate()
            if dm is not None:
                a = rand_autonomy_action()
                d0 = autonomy(dm, a, tripwire_autonomy_state(x["state"], now))
                d1 = autonomy(dm, a, tripwire_autonomy_state(y["state"], now))
                check(STRICT[d1["decision"]] >= STRICT[d0["decision"]], "MI-11 a reducing tripwire change never loosens autonomy", (ctx, a, d0, d1))
        check(all(i in first_b and first_b[i] <= k for i, k in first_a.items()), "MI-31 a reducing version never makes a tripwire fire later",
              (old_t, new_t, first_a, first_b))

def fuzz_tripwire_rules(n):
    """V-044, V-020, and V-042 for tripwires, against bounds the oracle reads by string surgery: ids sorted and unique;
    a counted threshold all digits and in [1, 1000]; a loss threshold of at most two decimals and at most the
    allocation; a proposed tripwire needs confirmation; removing one drops the delegations a version carries."""
    for _ in range(n):
        m = copy.deepcopy(base.BASES["research_equity"])
        alloc = rng.choice(["500", "10000"])
        m["capital"]["allocation_usd"] = alloc
        m["risk"].update({"max_order_usd": "100", "max_position_usd": "200", "max_gross_exposure_usd": "500"})
        tws = []
        for i in sorted(rng.sample(TW_IDS + ["a_trip"], rng.randint(1, 3))):
            metric = rng.choice(TW_METRICS)
            th = rng.choice(["1", "2", "999", "1000", "1001", "2.5", "0.01", "0.001", "499.99", "500", "500.01", "10000", "12.345"])
            tws.append({"id": i, "metric": metric, "threshold": th, "action": rng.choice(["end_delegations", "exits_only"])})
        if rng.random() < 0.2:
            tws.reverse()
        m["autonomy"]["tripwires"] = tws
        V.validate(m)
        ctx = dict(base.CTX)
        prov = rng.choice([None, {"source": "platform_proposed", "confirmed": True}, {"source": "platform_proposed", "confirmed": False}])
        if prov is not None:
            ctx["provenance"] = {"/autonomy/tripwires": prov}
        errs = semantic(m, ctx)[0]
        ids = [t["id"] for t in tws]

        def bad(t):
            th = t["threshold"]
            whole, _, frac = th.partition(".")
            if t["metric"] in ("consecutive_losing_exits", "new_instruments"):
                return frac != "" or not 1 <= int(whole) <= 1000
            return len(frac) > 2 or Fraction(th) > Fraction(alloc)
        want = any(a >= b for a, b in zip(ids, ids[1:])) or any(bad(t) for t in tws)
        check(("V-044" in errs) == want, "V-044 refuses exactly unsorted ids and out-of-range thresholds", (tws, alloc, errs))
        check(("V-020" in errs) == (prov is not None and not prov["confirmed"]), "V-020 a proposed tripwire is valid once confirmed", (prov, errs))
        check("V-022" not in errs and "V-038" not in errs, "§7 the platform may propose a tripwire", errs)
        d = copy.deepcopy(base.BASES["research_equity"])
        d["autonomy"]["tripwires"] = [{"id": "b_trip", "metric": "consecutive_losing_exits", "threshold": "3", "action": "exits_only"}]
        d["autonomy"]["delegations"] = [{"id": "d1", "lifts": "rule:large_orders", "when": {"field": "purpose", "op": "in", "value": ["increase", "open"]},
                                         "max_order_usd": "500", "max_orders": 2, "max_total_usd": "1000",
                                         "starts_at": fmt(DELEG_NOW), "expires_at": fmt(DELEG_NOW + timedelta(days=10)), "source_approval_id": None}]
        nxt = copy.deepcopy(d)
        change = rng.choice(["remove", "raise", "lower", "add", "soften"])
        tw = nxt["autonomy"]["tripwires"]
        if change == "remove":
            tw.clear()
        elif change == "raise":
            tw[0]["threshold"] = "4"
        elif change == "lower":
            tw[0]["threshold"] = "2"
        elif change == "soften":
            tw[0]["action"] = "end_delegations"
        else:
            tw.append({"id": "c_trip", "metric": "realized_loss_usd", "threshold": "50", "action": "end_delegations"})
        errs = semantic(nxt, dict(base.CTX, previous_version=d))[0]
        check(("V-042" in errs) == (change in ("remove", "raise", "soften")),
              "V-042 a version that removes or loosens a tripwire carries no delegation; one that adds or tightens does", (change, errs))

# ------------------------------------------------------------------ escalation (§6.1, §6.4; MI-21 to MI-25)
# The oracles below keep their own integer clock, deadlines, pending set, grant sets, and assertion ledger, and
# compute New York wall time with datetime.fromtimestamp; none of them calls the admission or re-validation code.
NYC = ZoneInfo("America/New_York")
ESC_T0 = 1_789_999_200
OWN_BAND_BP = {"us_equity": 100, "crypto": 200}
ESC_INSTRUMENTS = {base.ABC: "us_equity", base.BTC: "crypto"}
ESC_LABELS = ["rule:r1", "rule:r2", "default", "admission_ceiling"]
ACTOR_KINDS = ["system", "agent", "user", "broker", "platform_operator"]

def ts(s):
    return datetime.fromtimestamp(s, timezone.utc).strftime("%Y-%m-%dT%H:%M:%S") + ".000000000Z"

def scaled(p):
    """A decimal string as an integer count of 10^-8, by string surgery rather than Decimal."""
    neg = p.startswith("-")
    whole, _, frac = p.lstrip("-").partition(".")
    v = int(whole + frac.ljust(8, "0"))
    return -v if neg else v

def own_drift_ok(m_req, m_now, cls):
    if m_req is None or m_now is None:
        return False
    a, b = scaled(m_req), scaled(m_now)
    return abs(b - a) * 10000 <= OWN_BAND_BP[cls] * a

def own_step_up_ok(ev, at, env, seen):
    if not isinstance(ev, dict) or not isinstance(ev.get("assertion"), str) or not isinstance(ev.get("at_s"), int):
        return False
    return 0 <= at - ev["at_s"] <= 300 and ev["assertion"] not in seen and ev.get("method") == "cli_confirm" and env == "paper"

def wire(ev):
    """The fuzz's evidence as the journal carries it: `at_s` is the oracle's own integer copy."""
    if not isinstance(ev, dict):
        return ev
    out = {k: v for k, v in ev.items() if k != "at_s"}
    if isinstance(ev.get("at_s"), int):
        out["authenticated_at"] = ts(ev["at_s"])
    return out

def rand_mark(m_req, cls):
    if m_req is None or rng.random() < 0.08:
        return None
    band = D(OWN_BAND_BP[cls]) / 10000
    r = D(m_req)
    pick = rng.random()
    if pick < 0.25:
        return norm(r * (1 + band * rng.choice([1, -1])))
    if pick < 0.5:
        edge = r * (1 + band * rng.choice([1, -1]))
        return norm(edge + rng.choice([D("0.0001"), D("-0.0001")]) * (1 if edge > r else -1))
    return norm(r * (1 + D(rng.randint(-300, 300)) / 10000))

def rand_evidence(eff, seen_list, fresh_ids):
    k = rng.random()
    if k < 0.08:
        return None
    if k < 0.12:
        return rng.choice([{"assertion": "a-malformed"}, {"authenticated_at": "yesterday", "method": "cli_confirm"}, "cli_confirm"])
    assertion = rng.choice(seen_list) if seen_list and rng.random() < 0.15 else f"a{next(fresh_ids)}"
    return {"assertion": assertion, "at_s": eff - rng.choice([-1, 0, 1, 150, 299, 300, 301, 900]),
            "method": "cli_confirm" if rng.random() < 0.95 else "passkey"}

def rand_now(bound):
    cls = bound["asset_class"]
    return {"mandate_version": bound["mandate_version"] if rng.random() < 0.9 else "sha256:v2",
            "mode": "normal" if rng.random() < 0.85 else rng.choice(["exits_only", "paused", "stopped"]),
            "instrument_restricted": rng.random() < 0.07, "in_working_universe": rng.random() > 0.05,
            "classification": rng.choice([{"decision": "auto", "by": "default"}, {"decision": "ask", "by": bound["decided_by"]},
                                          {"decision": "ask", "by": bound["decided_by"]}, {"decision": "ask", "by": "rule:other"},
                                          {"decision": "deny", "by": "rule:d"}]),
            "dry_run": {"verdict": "allow", "reason": None} if rng.random() < 0.85 else {"verdict": "deny", "reason": "concentration_limit"},
            "mark": rand_mark((bound["reference_mark"] or {}).get("price"), cls)}

def own_acts(bound, now):
    c = now["classification"]
    return (now["mandate_version"] == bound["mandate_version"] and now["mode"] == "normal"
            and not now["instrument_restricted"] and now["in_working_universe"]
            and c["decision"] != "deny" and not (c["decision"] == "ask" and c["by"] != bound["decided_by"])
            and now["dry_run"]["verdict"] == "allow"
            and own_drift_ok((bound["reference_mark"] or {}).get("price"), now["mark"], bound["asset_class"]))

POLICY_CEILINGS = [None, None, "24.875", "100", "101", "303", "1000", "43000", "129000"]

def own_quorum(bound, policy):
    """Check 7's requirement, computed apart from the model: the order value and the policy ceiling in 10^-16 units,
    independence if the request or the oracle's own record of the policy requires it, and the larger count."""
    order = scaled(bound["qty"]) * scaled(bound["limit_price"])
    over = policy["ceiling"] is not None and order > scaled(policy["ceiling"]) * 10 ** 8
    return max(bound["approvers_required"], 2 if over else 1), bound["independent_required"] or policy["independent"]

TABLE = {("open", "ApprovalDelivered"): "open", ("open", "refused"): "open", ("open", "counted"): "open",
         ("open", "skipped"): "done", ("open", "approved"): "granted", ("granted", "act"): "acted",
         ("granted", "skip"): "done", ("acted", "IntentProposed"): "done", ("open", "ApprovalTimedOut"): "done",
         ("open", "ApprovalCanceled"): "done", ("done", "refused_not_pending"): "done"}

def transition_label(e):
    t = e["type"]
    if t == "ApprovalResponded":
        if e["result"] == "refused":
            return "refused_not_pending" if e["reason"] == "not_pending" else "refused"
        if e["result"] == "counted":
            return "counted"
        return e["verdict"]
    if t == "ApprovalRevalidated":
        return e["result"]
    return t

def fuzz_escalation(n):
    """MI-21 to MI-25 (EI-1 to EI-7, EI-10, EI-11, EI-13 to EI-16) over random asks, ticks, responses, re-tailed
    responses (some read in a step whose mode is exits-only or stricter, which cancels first), cancellations, a
    cancellation batched with responses, and workspace `PolicyChanged` events between a
    request and its responses. Every response the oracle judges timely, human, listed, delivered, matching, freshly
    and singly stepped-up, in the quorum of the stricter of the bound requirement and the oracle's own record of the
    current policy, and passing every re-validation value must act, and no other may; every grant that reaches check 7,
    and no other response, records that quorum on its `ApprovalResponded`; every approval ends in exactly one terminal
    event of the transition table."""
    for _ in range(n):
        env = "paper" if rng.random() < 0.85 else "live"
        timeout_s = rng.choice([30, 60, 300, 600])
        inbox = 1 if rng.random() < 0.85 else 0
        push, qh = (["email"], rng.choice([None, {"start": "00:00", "end": "23:59"}])) if not inbox or rng.random() < 0.3 else ([], None)
        ctx = {"approvers": {"u1", "u2"}, "author": "u1", "environment": env, "timeout_s": timeout_s,
               "inbox": inbox, "push_channels": push, "quiet_hours": qh}
        st = escalation_fold([], ts(ESC_T0))
        journal, history, ids, fresh = [], [], itertools.count(1), itertools.count(1)
        own = {"clock": ESC_T0, "pending": {}, "seen": [], "sources": set(), "policy": {"independent": False, "ceiling": None}}
        bound_of, deadline_of, hash_of, drafts = {}, {}, {}, []
        for _ in range(rng.randint(4, 28)):
            just_counted = any(d["type"] == "ApprovalResponded" and d["result"] == "counted" for d in drafts)
            if rng.random() < (0.6 if just_counted else 0.1):
                independent, ceiling = rng.random() < (0.8 if just_counted else 0.4), rng.choice(POLICY_CEILINGS)
                journal.append({"type": "PolicyChanged", "independent_approval_required": independent,
                                "two_approver_above_usd": ceiling, "clock": ts(own["clock"])})
                escalation_apply(st, journal[-1])
                own["policy"] = {"independent": independent, "ceiling": ceiling}
            k = rng.random()
            if k < 0.2:
                inst = rng.choice(list(ESC_INSTRUMENTS))
                ref_price = rng.choice(["100", "250.5", "43210.25", "0.5", None])
                bound = {"instrument": inst, "asset_class": ESC_INSTRUMENTS[inst], "side": "buy",
                         "qty": rng.choice(["1", "3", "0.25", "12"]), "limit_price": rng.choice(["99.5", "101", "43000"]),
                         "purpose": rng.choice(["open", "increase"]), "mandate_version": "sha256:v1",
                         "decided_by": rng.choice(ESC_LABELS), "combined_score": "0.7",
                         "reference_mark": None if ref_price is None else {"price": ref_price, "seq": rng.randint(1, 99)},
                         "approvers_required": rng.choice([1, 1, 2]), "independent_required": rng.random() < 0.3,
                         "timeout_s": timeout_s}
                a = f"ap{next(ids)}"
                inp = {"kind": "ask", "approval": a, "bound": bound}
            elif k < 0.38:
                ahead = [d - own["clock"] for d in deadline_of.values() if d > own["clock"]]
                step = rng.choice(ahead) if ahead and rng.random() < 0.4 else rng.choice([0, 1, 30, 120, 299, 400])
                inp = {"kind": "tick", "at": ts(own["clock"] + step)}
            elif k < 0.84 or not history:
                inp = None
            elif k < 0.9:
                inp = copy.deepcopy(rng.choice(history))
            elif k < 0.95:
                inp = {"kind": "cancel", "reason": rng.choice(CANCEL_REASONS)}
            else:
                inp = {"kind": "batch", "reason": rng.choice(CANCEL_REASONS), "responses": []}
            if inp is None or inp["kind"] == "batch":
                known = list(own["pending"]) if own["pending"] and rng.random() < 0.9 else list(bound_of) + ["ap-unknown"]
                resps = []
                for _ in range(1 if inp is None else rng.randint(1, 2)):
                    a = rng.choice(known)
                    dl = deadline_of.get(a, own["clock"] + 60)
                    sub = rng.choice([own["clock"] - 100, own["clock"] - 1, own["clock"], own["clock"] + 1, dl - 1, dl, dl + 5])
                    eff = max(sub, own["clock"])
                    resps.append({"source": f"ctl{next(ids)}", "approval": a, "actor_kind": "user" if rng.random() < 0.8 else rng.choice(ACTOR_KINDS),
                                  "responder": rng.choice(["u1", "u2", "u2", "u3"]), "verdict": "approved" if rng.random() < 0.8 else "skipped",
                                  "content_hash": hash_of.get(a, "sha256:none") if rng.random() < 0.9 else "sha256:" + "f" * 64,
                                  "submitted_at": sub, "step_up": rand_evidence(eff, own["seen"], fresh)})
                nows = [rand_now(bound_of[r["approval"]]) if r["approval"] in bound_of else rand_now(
                        {"mandate_version": "sha256:v1", "decided_by": "default", "asset_class": "crypto", "reference_mark": None}) for r in resps]
                if inp is None:
                    inp = {"kind": "response", "response": resps[0], "now": nows[0]}
                else:
                    inp["responses"], inp["now"] = resps, nows[0]
                history.append(inp)
            fed = copy.deepcopy(inp)
            for r in ([fed["response"]] if fed["kind"] == "response" else fed.get("responses", [])):
                r["submitted_at"], r["step_up"] = ts(r["submitted_at"]), wire(r["step_up"])
            if fed["kind"] == "tick":
                journal.append({"type": "ClockAdvanced", "clock": fed["at"]})
                escalation_apply(st, journal[-1])
            drafts = escalation_step(st, fed, ctx)
            for d in drafts:
                escalation_apply(st, d)
            journal += drafts
            expected, granted, quorate, applied = {}, {}, {}, {}
            if inp["kind"] == "ask":
                asked = [d for d in drafts if d["type"] == "ApprovalRequested"]
                check(bool(asked) == (not own["pending"]), "MI-25 one pending risk-adding approval per agent", (own["pending"], asked))
                if asked:
                    a, b = inp["approval"], inp["bound"]
                    bound_of[a], deadline_of[a], hash_of[a] = b, own["clock"] + timeout_s, asked[0]["content_hash"]
                    ny = datetime.fromtimestamp(own["clock"], NYC)
                    delivered = bool(inbox) or (qh is None or ny.hour * 60 + ny.minute == 23 * 60 + 59)
                    own["pending"][a] = {"grants": set(), "delivered": delivered}
            elif inp["kind"] == "tick":
                own["clock"] = max(own["clock"], int(T(inp["at"]).timestamp()))
                for a in [a for a in own["pending"] if deadline_of[a] <= own["clock"]]:
                    del own["pending"][a]
            tightened = inp["kind"] == "response" and inp["now"]["mode"] != "normal"
            if inp["kind"] in ("cancel", "batch") or tightened:
                own["pending"].clear()
            for r in ([inp["response"]] if inp["kind"] == "response" else inp.get("responses", [])):
                if r["source"] in own["sources"]:
                    expected[r["source"]] = None
                    continue
                own["sources"].add(r["source"])
                a, eff = r["approval"], max(r["submitted_at"], own["clock"])
                p = own["pending"].get(a)
                ok = (p is not None and eff < deadline_of[a] and r["actor_kind"] == "user" and r["responder"] in ("u1", "u2")
                      and p["delivered"] and r["content_hash"] == hash_of[a])
                acts = granted[r["source"]] = quorate[r["source"]] = False
                need, independent = own_quorum(bound_of[a], own["policy"]) if a in bound_of else (1, False)
                if ok and r["verdict"] == "skipped":
                    del own["pending"][a]
                elif ok and own_step_up_ok(r["step_up"], eff, env, own["seen"]):
                    applied[r["source"]] = {"required": need, "independent": independent}
                    if r["responder"] not in p["grants"] and not (independent and r["responder"] == "u1"):
                        granted[r["source"]] = True
                        p["grants"].add(r["responder"])
                        if len([g for g in p["grants"] if not (independent and g == "u1")]) >= need:
                            quorate[r["source"]] = True
                            del own["pending"][a]
                            acts = own_acts(bound_of[a], inp["now"])
                if isinstance(r["step_up"], dict) and isinstance(r["step_up"].get("assertion"), str):
                    own["seen"].append(r["step_up"]["assertion"])
                expected[r["source"]] = acts
            last, last_approval, reval_act, produced = None, None, False, {}
            for d in drafts:
                if d["type"] == "ApprovalResponded":
                    check((d["verdict"] == "approved" and d["result"] in ("admitted", "counted")) == granted.get(d["source"], False),
                          "MI-24 a grant counts exactly when a listed human shown this content steps up freshly and once",
                          (d, granted.get(d["source"])))
                    check((d["verdict"] == "approved" and d["result"] == "admitted") == quorate.get(d["source"], False),
                          "MI-24 a grant is admitted exactly at the stricter of the bound and the current policy quorum",
                          (d, own["policy"], bound_of.get(d["approval"], {}).get("approvers_required"), quorate.get(d["source"])))
                    check(d.get("quorum") == applied.get(d["source"]),
                          "MI-24 ApprovalResponded records the approver count and independence check 7 applied, for exactly the grants that reach it",
                          (d, own["policy"], applied.get(d["source"])))
                    last, last_approval, reval_act = d["source"], d["approval"], False
                    produced.setdefault(last, False)
                elif d["type"] == "ApprovalRevalidated":
                    reval_act = d["result"] == "act" and d["approval"] == last_approval
                elif d["type"] == "IntentProposed":
                    check(reval_act and d["approval"] == last_approval, "MI-21 an intent follows only a grant re-validated to act",
                          (inp["kind"], d))
                    reval_act = False
                    if last is not None:
                        produced[last] = True
                    b = bound_of.get(d["approval"], {})
                    bound_action = {
                        "instrument_id": b.get("instrument"),
                        "side": b.get("side"),
                        "order_type": "limit",
                        "tif": "day" if b.get("asset_class") == "us_equity" else "gtc",
                        "qty": b.get("qty"),
                        "limit_price": b.get("limit_price"),
                        "purpose": b.get("purpose"),
                        "mandate_version": b.get("mandate_version"),
                    }
                    check(
                        {field: d[field] for field in bound_action} == bound_action,
                        "MI-22 the intent equals the bound fields",
                        (d, b),
                    )
            for src, want in expected.items():
                if want is None:
                    check(src not in produced, "MI-21 a re-tailed control-stream event is copied once", (src, drafts))
                else:
                    check(produced.get(src, False) == want, "MI-21 a response acts exactly when every check passes",
                          (inp["kind"], src, want, produced.get(src), [d for d in drafts if d["type"] != "ApprovalDelivered"]))
            if inp["kind"] in ("cancel", "batch") or tightened:
                check(not st["pending"], "MI-21 no approval outlives the tightening or version that cancelled it", st["pending"])
            check(proposal_route(st["pending"], rng.choice(sorted(REDUCING))) == "handed",
                  "MI-23 no exit waits on an approval", st["pending"])
            check((proposal_route(st["pending"], "open") == "awaiting_approval") == bool(own["pending"]),
                  "MI-25 a risk-adding proposal waits exactly while an approval is pending", (st["pending"], own["pending"]))
        journal.append({"type": "ClockAdvanced", "clock": ts(own["clock"] + 10 ** 6)})
        escalation_apply(st, journal[-1])
        final = escalation_step(st, {"kind": "tick", "at": journal[-1]["clock"]}, ctx)
        for d in final:
            escalation_apply(st, d)
        journal += final
        for a in bound_of:
            state = "start"
            for e in (e for e in journal if e.get("approval") == a):
                if e["type"] == "ApprovalRequested":
                    nxt = "open" if state == "start" else None
                else:
                    nxt = TABLE.get((state, transition_label(e)))
                check(nxt is not None, "MI-21 every move is in the approval transition table", (a, state, e))
                state = nxt or state
            check(state == "done", "MI-21 every approval ends in exactly one terminal event", (a, state))
        replay = escalation_fold(journal, ts(ESC_T0))
        check(replay["pending"] == st["pending"] and replay["used"] == st["used"] and replay["policy"] == st["policy"],
              "MI-21 replay folds to the same approvals", None)

def run_policy_quorum(bound, script):
    """One approval through `script`, a list of ("policy", independent, ceiling) and ("grant", responder) moves, all
    with fresh valid step-up and inside the deadline. Returns the model's results and the oracle's, move by move."""
    ctx = {"approvers": {"u1", "u2"}, "author": "u1", "environment": "paper", "timeout_s": 600,
           "inbox": 1, "push_channels": [], "quiet_hours": None}
    st = escalation_fold([], ts(ESC_T0))
    for d in escalation_step(st, {"kind": "ask", "approval": "ap1", "bound": bound}, ctx):
        escalation_apply(st, d)
    content_hash = next(iter(st["pending"].values()))["content_hash"]
    policy, grants, open_, got, want = {"independent": False, "ceiling": None}, set(), True, [], []
    for i, move in enumerate(script):
        if move[0] == "policy":
            escalation_apply(st, {"type": "PolicyChanged", "independent_approval_required": move[1],
                                  "two_approver_above_usd": move[2], "clock": ts(ESC_T0)})
            policy = {"independent": move[1], "ceiling": move[2]}
            continue
        who = move[1]
        resp = {"source": f"ctl{i}", "approval": "ap1", "actor_kind": "user", "responder": who, "verdict": "approved",
                "content_hash": content_hash, "submitted_at": ts(ESC_T0),
                "step_up": {"assertion": f"q{i}", "authenticated_at": ts(ESC_T0), "method": "cli_confirm"}}
        now = {"mandate_version": bound["mandate_version"], "mode": "normal", "instrument_restricted": False,
               "in_working_universe": True, "classification": {"decision": "ask", "by": bound["decided_by"]},
               "dry_run": {"verdict": "allow", "reason": None}, "mark": bound["reference_mark"]["price"]}
        drafts = escalation_step(st, {"kind": "response", "response": resp, "now": now}, ctx)
        for d in drafts:
            escalation_apply(st, d)
        got.append(next(d["result"] for d in drafts if d["type"] == "ApprovalResponded"))
        need, independent = own_quorum(bound, policy)
        if not open_:
            want.append("refused")
        elif who in grants or (independent and who == "u1"):
            want.append("refused")
        else:
            grants.add(who)
            quorate = len([g for g in grants if not (independent and g == "u1")]) >= need
            want.append("admitted" if quorate else "counted")
            open_ = not quorate
    return got, want

def fuzz_policy_quorum(n):
    """MI-24 and DEC-173 item 13: check 7 takes the stricter of the bound requirement and the workspace policy overlay
    current at the response, so no `PolicyChanged` between request and response loosens a pending approval. The oracle
    keeps its own record of the policy and its own grant set; the pinned scripts are the review's three cases."""
    base_bound = {"instrument": base.ABC, "asset_class": "us_equity", "side": "buy", "qty": "3", "limit_price": "101",
                  "purpose": "open", "mandate_version": "sha256:v1", "decided_by": "default", "combined_score": "0.7",
                  "reference_mark": {"price": "101", "seq": 1}, "timeout_s": 600}
    pinned = [
        ("maker-checker turned on while pending binds it, and the author's counted grant stops counting",
         dict(base_bound, approvers_required=2, independent_required=False),
         [("grant", "u1"), ("policy", True, None), ("grant", "u2")], ["counted", "counted"]),
        ("a lowered ceiling while pending raises the count to two",
         dict(base_bound, approvers_required=1, independent_required=False),
         [("policy", False, "101"), ("grant", "u1")], ["counted"]),
        ("a policy loosened while pending leaves the bound requirement in force",
         dict(base_bound, approvers_required=2, independent_required=True),
         [("policy", False, None), ("grant", "u1"), ("grant", "u2")], ["refused", "counted"]),
    ]
    for title, bound, script, want in pinned:
        got, oracle = run_policy_quorum(bound, script)
        check(got == want == oracle, f"DEC-173 item 13 {title}", (got, want, oracle))
    for _ in range(n):
        bound = dict(base_bound, qty=rng.choice(["1", "3", "0.25", "12"]), limit_price=rng.choice(["99.5", "101", "43000"]),
                     approvers_required=rng.choice([1, 2]), independent_required=rng.random() < 0.4)
        script = [("policy", rng.random() < 0.5, rng.choice(POLICY_CEILINGS)) if rng.random() < 0.4
                  else ("grant", rng.choice(["u1", "u1", "u2"])) for _ in range(rng.randint(1, 6))]
        got, want = run_policy_quorum(bound, script)
        check(got == want, "MI-24 check 7 never loosens a pending approval under a policy change", (bound, script, got, want))

def fuzz_drift(n):
    """MI-22's drift band against a scaled-integer oracle, with the exact band edge and one unit either side."""
    for _ in range(n):
        cls = rng.choice(["us_equity", "crypto"])
        m_req = rng.choice(["100", "250.5", "43210.25", "0.5", "7", "19.99"])
        m_now = rand_mark(m_req, cls) if rng.random() < 0.95 else None
        check(within_drift(m_req, m_now, cls) == own_drift_ok(m_req, m_now, cls), "MI-22 drift is inside the band exactly as the integers say",
              (m_req, m_now, cls))
    for cls, m_req, inside, outside in [("us_equity", "100", ["101", "99"], ["101.0001", "98.9999"]),
                                        ("crypto", "100", ["102", "98"], ["102.0001", "97.9999"])]:
        for m in inside:
            check(within_drift(m_req, m, cls), "MI-22 a move exactly at the band is inside", (cls, m))
        for m in outside:
            check(not within_drift(m_req, m, cls), "MI-22 one unit beyond the band is outside", (cls, m))

def fuzz_ask_budget(n):
    """MI-25: the oracle counts asks per America/New_York day from integer instants, across both 2026 DST changes and the
    hours where the UTC and New York dates differ, with its own skip and timeout windows."""
    days = [datetime(2026, 3, 8, 12, tzinfo=NYC), datetime(2026, 11, 1, 12, tzinfo=NYC), datetime(2026, 9, 22, 12, tzinfo=NYC)]
    for _ in range(n):
        noon = int(rng.choice(days).timestamp())
        evs = []
        for _ in range(rng.randint(0, rng.choice([4, 24]))):
            t = noon + rng.randint(-14 * 3600, 11 * 3600 + 3599)
            kind = rng.choices(["requested", "owner_skipped", "timed_out", "version_applied"], [8, 2, 2, 1])[0]
            evs.append({"event": kind, "at_s": t, "instrument": rng.choice(["A", "B"]), "timeout_s": rng.choice([30, 300, 600])})
        evs.sort(key=lambda e: e["at_s"])
        last = evs[-1]["at_s"] if evs else noon
        timeouts = [e for e in evs if e["event"] == "timed_out"]
        q = rng.choice([last, last + 1, noon + 11 * 3600 + 3599] + [e["at_s"] + e["timeout_s"] + rng.choice([-1, 0]) for e in timeouts])
        q = max(q, last)
        inst = rng.choice(["A", "B"])
        if timeouts and rng.random() < 0.5:
            edge = rng.choice(timeouts)
            q, inst = max(edge["at_s"] + edge["timeout_s"] + rng.choice([-1, 0]), last), edge["instrument"]
        day = datetime.fromtimestamp(q, NYC).date()
        same = lambda e: datetime.fromtimestamp(e["at_s"], NYC).date() == day
        asked = len([e for e in evs if e["event"] == "requested" and same(e)])
        skipped = False
        for e in evs:
            if e["event"] == "owner_skipped" and e["instrument"] == inst and same(e):
                skipped = True
            if e["event"] == "version_applied":
                skipped = False
        recent = any(e["event"] == "timed_out" and e["instrument"] == inst and e["at_s"] <= q < e["at_s"] + e["timeout_s"] for e in evs)
        want = "budget" if asked >= 10 else "skipped_today" if skipped else "recent_timeout" if recent else None
        ledger = [{"event": e["event"], "at": ts(e["at_s"]), "instrument": e["instrument"], "timeout_s": e["timeout_s"]} for e in evs]
        got = ask_permit(ledger, inst, ts(q))
        check(got == want, "MI-25 the ask budget and suppression windows", (want, got, ts(q), [(e["event"], ts(e["at_s"])) for e in evs]))

def fuzz_quiet_hours(n):
    """MI-25: quiet hours suppress a push inside [start, end) New York wall time and never the inbox. The oracle walks the
    window minute by minute; the pinned instants are 23:00 and 07:00 in both DST states under 23:00 to 07:00."""
    for winter, summer in [(datetime(2026, 1, 15, 23, 0, tzinfo=NYC), datetime(2026, 7, 15, 23, 0, tzinfo=NYC)),
                           (datetime(2026, 1, 15, 7, 0, tzinfo=NYC), datetime(2026, 7, 15, 7, 0, tzinfo=NYC))]:
        for at in (winter, summer):
            want = "suppressed_quiet_hours" if at.hour == 23 else "delivered"
            check(deliver_now("push", {"start": "23:00", "end": "07:00"}, ts(int(at.timestamp()))) == want,
                  "MI-25 quiet hours are New York wall time in both DST states", (at, want))
            check(deliver_now("cli_inbox", {"start": "23:00", "end": "07:00"}, ts(int(at.timestamp()))) == "delivered",
                  "MI-25 quiet hours never suppress the inbox", at)
    for _ in range(n):
        s, e = rng.randrange(1440), rng.randrange(1440)
        if s == e:
            continue
        window, mnt = set(), s
        while mnt != e:
            window.add(mnt)
            mnt = (mnt + 1) % 1440
        qh = {"start": f"{s // 60:02d}:{s % 60:02d}", "end": f"{e // 60:02d}:{e % 60:02d}"}
        at = ESC_T0 + rng.randrange(365 * 86400)
        local = datetime.fromtimestamp(at, NYC)
        for channel in ("push", "cli_inbox"):
            want = "suppressed_quiet_hours" if channel == "push" and local.hour * 60 + local.minute in window else "delivered"
            check(deliver_now(channel, qh, ts(at)) == want, "MI-25 quiet hours suppress exactly the pushes inside the window",
                  (channel, qh, local))

def fuzz_owner_controls(n):
    """MI-23: pause always applies; resume, stop, and acknowledge are judged when processed and an owner exit when
    committed; a refused owner exit loses only its privilege and is still routed; a kill switch with any evidence, or
    none, stops and flattens every position, selling equities outside the session only with valid evidence."""
    fresh = itertools.count(1)
    for _ in range(n):
        env = "paper" if rng.random() < 0.85 else "live"
        committed = ESC_T0 + rng.randrange(86400)
        processed = committed + rng.choice([0, 10, 150, 299, 300, 301, 3600])
        seen = [f"old{i}" for i in range(rng.randint(0, 2))]
        ev = rand_evidence(committed, seen, fresh)
        used = set(seen)
        kind = rng.choice(["pause", "resume", "stop", "acknowledge", "owner_exit"])
        got = owner_command(kind, wire(ev), ts(committed), ts(processed), env, used)
        at = committed if kind == "owner_exit" else processed
        want = kind == "pause" or own_step_up_ok(ev, at, env, seen)
        check((got["result"] == "apply") == want, "MI-23 each owner control is judged at its own moment", (kind, ev, committed, processed, env, got))
        cls = rng.choice(["us_equity", "crypto"])
        session = rng.choice(["regular", "pre_market", "after_hours"]) if cls == "us_equity" else "crypto"
        mode = rng.choice(["normal", "exits_only", "paused"])
        confirmed = rng.random() < 0.6
        auth = owner_command("owner_exit", wire(ev), ts(committed), ts(processed), env, used)
        prop = {"purpose": "owner_exit", "instrument": base.ABC if cls == "us_equity" else base.BTC, "qty": "1", "limit_price": "100"}
        out = owner_exit_order(base.swing, {}, prop, mode, session, cls, confirmed, auth)
        privileged = confirmed and own_step_up_ok(ev, committed, env, seen)
        want_verdict = ("hold" if mode == "paused" else
                        "defer" if cls == "us_equity" and session != "regular" and not privileged else "allow")
        check(out["verdict"] == want_verdict, "MI-23 a refused owner exit loses only its privilege and is still routed",
              (mode, session, cls, confirmed, ev, out))
        positions = [{"agent": "a1", "instrument": f"E{i}", "asset_class": "us_equity", "qty": "2"} for i in range(rng.randint(0, 2))]
        positions += [{"agent": "a1", "instrument": "BTC", "asset_class": "crypto", "qty": "0.5"}] * (rng.random() < 0.5)
        positions += [{"agent": "a2", "instrument": "OTHER", "asset_class": "us_equity", "qty": "4"}]
        ks_session = rng.choice(["regular", "pre_market", "after_hours"])
        inp = {"agent": "a1", "owner_confirmed_bid": confirmed, "confirmed_bid": "100", "max_exit_offset": "0.02",
               "owner_floor_price": None, "session": ks_session, "open_orders": [], "agent_positions": positions}
        ks = owner_kill_switch(inp, wire(ev), ts(committed), env, used)
        mine = sorted(p["instrument"] for p in positions if p["agent"] == "a1")
        sold = {s["instrument"] for s in ks["sells"]}
        waiting = {s["instrument"] for s in ks["deferred_sells"]}
        check(ks["mode_applied_first"] == "stopped", "MI-23 a kill switch with any evidence stops the agent", (ev, ks))
        check(sorted(sold | waiting) == mine and not sold & waiting, "MI-23 a kill switch flattens every position of its agent",
              (ev, positions, ks))
        ks_privileged = confirmed and own_step_up_ok(ev, committed, env, seen)
        for p in positions:
            if p["agent"] == "a1" and p["asset_class"] == "us_equity" and ks_session != "regular":
                check((p["instrument"] in sold) == ks_privileged,
                      "MI-23 only valid evidence as committed sells equities outside the session", (ev, ks_session, confirmed, ks))

def fuzz_stop_limit_offset(n):
    """V-008 and W-002 under DEC-539: the offset is required when protection is enabled and any allowed asset class is
    one the connection's profile protects with a stop-limit (`stop_limit_asset_classes`; absent or null, every allowed
    class, so validation fails closed), and the worst case adds it exactly then. A version may move from schema version
    1 to 2 but never back (V-031). A version-1 document reads `crypto_stop_limit_offset` as the offset, so the same
    mandate written at either version validates alike, a move between versions with the same offset is neutral and
    changes no path, and a larger offset across the move is increasing on `/protection/stop_limit_offset` alone. The
    oracle computes the required set and the figure itself, and builds the version-2 document by hand."""
    for _ in range(n):
        m = copy.deepcopy(rng.choice([base.btc, base.swing]))
        m["universe"]["asset_classes"] = rng.choice([["us_equity"], ["crypto"], ["crypto", "us_equity"]])
        p = m["protection"]
        p["enabled"] = rng.random() < 0.8
        if not p["enabled"] and rng.random() < 0.5:
            p["stop_distance"] = p["take_profit_distance"] = None
        offset = rng.choice([None, "0.005", "0.01"])
        p["crypto_stop_limit_offset"] = offset
        ctx = dict(base.CTX)
        classes = rng.choice(["absent", None, [], ["crypto"], ["us_equity"], ["crypto", "us_equity"]])
        if classes == "absent":
            del ctx["stop_limit_asset_classes"]
        else:
            ctx["stop_limit_asset_classes"] = classes
        protected = set(m["universe"]["asset_classes"]) if classes in ("absent", None) else set(classes)
        needed = bool(set(m["universe"]["asset_classes"]) & protected)
        v2 = copy.deepcopy(m)
        v2["mandate_schema_version"] = 2
        v2["protection"] = {"enabled": p["enabled"], "stop_distance": p["stop_distance"],
                            "take_profit_distance": p["take_profit_distance"], "stop_limit_offset": offset}
        if not (V.is_valid(m) and V.is_valid(v2)):
            continue
        if p["enabled"]:
            v008 = needed and offset is None
            pos = min(D(m["risk"]["max_position_usd"]), D(m["risk"]["max_position_fraction"]) * D(m["capital"]["allocation_usd"]))
            figure = pos * (D(p["stop_distance"]) + (D(offset) if needed and offset else D(0)))
        else:
            v008 = any(x is not None for x in (p["stop_distance"], p["take_profit_distance"], offset))
            figure = None
        for doc in (m, v2):
            errs, _ = semantic(doc, ctx)
            check(("V-008" in errs) == v008, "V-008 requires the offset exactly where a stop-limit protects",
                  (doc["mandate_schema_version"], m["universe"]["asset_classes"], classes, p["enabled"], offset, errs))
            got = worst_case(doc, ctx)["one_position_at_stop_usd"]
            check((got is None and figure is None) or (got is not None and figure is not None and D(got) == figure),
                  "W-002's figure adds the offset exactly where a stop-limit protects", (doc["protection"], classes, got, figure))
        check(classify(m, v2) == ("neutral", []), "moving to version 2 with the same offset changes nothing",
              (m["protection"], classify(m, v2)))
        check("V-031" in semantic(m, dict(ctx, previous_version=v2))[0], "V-031 refuses a move back to version 1",
              (m["protection"], classes))
        check("V-031" not in semantic(v2, dict(ctx, previous_version=m))[0], "V-031 allows the move to version 2",
              (m["protection"], classes))
        if p["enabled"] and offset is not None:
            raised = copy.deepcopy(v2)
            raised["protection"]["stop_limit_offset"] = norm(D(offset) + D("0.005"))
            check(classify(m, raised) == ("risk_increasing", ["/protection/stop_limit_offset"]),
                  "a larger offset across the move is increasing on the one path", (offset, classify(m, raised)))

def fuzz_content(n):
    """§6.4 content and rule 6: exactly the nine keys; the trigger's rule is the owner's confirmed rule verbatim, or
    null for the default and the three ceilings; the trigger names who asked and the client (DEC-185); the choices end
    with exactly the delegation shapes offered (DEC-181); every bound field moves the hash; a notification carries no
    sentinel of the request."""
    m = copy.deepcopy(base.swing)
    for _ in range(n):
        m["autonomy"]["rules"] = [rand_rule(i) for i in range(rng.randint(0, 3))]
        labels = [f"rule:{r['id']}" for r in m["autonomy"]["rules"]] + ["default", "admission_ceiling", "client_ceiling", "review_ceiling"]
        requested_by = rng.choice(["agent", "owner", "client"])
        shapes = rng.choice([[], ["like_this_until_close"], ["like_this_until_close", "this_instrument", "this_kind"]])
        req = {"approval": "01J" + "SENTINELAPPROVAL"[:10].upper() + "0" * 13, "instrument": "SENTINELINSTR", "asset_class": "us_equity",
               "side": "buy", "qty": "777.77", "limit_price": "31415.9", "purpose": "open", "mandate_version": "sha256:sentinelversion",
               "decided_by": rng.choice(labels), "combined_score": "0.4242", "reference_mark": {"price": "27182.8", "seq": 4242},
               "deadline": "2026-09-22T14:10:00.000000000Z", "approvers_required": rng.choice([1, 2]), "independent_required": rng.random() < 0.5,
               "requested_by": requested_by, "client": rng.choice([None, "SENTINELCLIENT"]), "delegation_shapes": shapes}
        figures = {"agent_equity": "5000", "position_usd_after": "123456.7", "gross_usd_after": "234567.8", "bought_today_usd": "345678.9",
                   "drawdown": "0.0123", "daily_pnl_fraction": "-0.0456"}
        content = approval_content(m, req, figures)
        check(tuple(content) == CONTENT_KEYS, "§6.4 the content object has exactly the nine keys", list(content))
        rule = next((r for r in m["autonomy"]["rules"] if f"rule:{r['id']}" == req["decided_by"]), None)
        check(content["trigger"]["rule"] == rule, "§6.4 the trigger shows the owner's confirmed rule verbatim", (req["decided_by"], content["trigger"]))
        expected_client = req["client"] if requested_by == "client" else None
        check(content["trigger"]["requested_by"] == requested_by and content["trigger"]["client"] == expected_client,
              "§6.4 the trigger names who asked, and the client only for a client-requested order (DEC-185)", content["trigger"])
        check(content["choices"][:2] == ["approve", "skip"] and content["choices"][2:] == shapes,
              "§6.4 choices are approve and skip, then exactly the delegation shapes offered (DEC-181)", content["choices"])
        note = canon(approval_notification(req))
        check(set(approval_notification(req)) == {"subject", "text"} and not any(
              s in note for s in ("SENTINEL" + "INSTR", "SENTINEL" + "CLIENT", "777.77", "31415.9", "sentinelversion", "0.4242", "27182.8", "2026-09-22")),
              "rule 6 a notification carries only the opaque id and generic text", note)
        field = rng.choice(["instrument", "qty", "limit_price", "purpose", "mandate_version", "decided_by", "reference_mark", "approvers_required",
                            "requested_by", "client", "delegation_shapes"])
        ctx = {"timeout_s": 600, "inbox": 1, "push_channels": [], "quiet_hours": None}
        st = escalation_fold([], ts(ESC_T0))
        bound = {k: v for k, v in req.items() if k not in ("approval", "deadline")}
        other = dict(bound, **{field: {"instrument": "X2", "qty": "1", "limit_price": "2", "purpose": "increase", "mandate_version": "sha256:v9",
                                       "decided_by": "rule:zz", "reference_mark": None, "approvers_required": 3,
                                       "requested_by": "owner" if requested_by != "owner" else "client",
                                       "client": "OTHERCLIENT", "delegation_shapes": shapes + ["this_kind_extra"]}[field]})
        h = [escalation_step(st, {"kind": "ask", "approval": "ap1", "bound": b}, ctx)[0]["content_hash"] for b in (bound, other)]
        check(h[0] != h[1], "§6.4 every bound field moves the content hash", field)

def fuzz_independence_floor(n):
    """V-047 (DEC-411, DEC-444): under `independent_approval_required` a workspace of fewer than two users is refused at
    validation, a user count that is absent counts as one (rule 3), and the rule touches no other verdict, except that a
    new version §9.2 rates risk-reducing against the agent's current version passes: a whole schema-valid document
    whose canonical hash is the current `mandate_version` the platform supplies. The oracle names the lone workspaces
    as a set, the absent count among them; it builds each previous version so its §9.2 label is known by construction,
    never by calling `classify`: one maximum raised behind the draft (reducing), the default made laxer behind it
    (reducing, not a maximum), a rename or other quiet hours (neutral), the draft's maximum raised (increasing), and one
    maximum raised behind it with another lowered (increasing). A forged predecessor (a reducing-looking document while
    the current version is another) and one with no current version are not the agent's version, so they are refused,
    and so is an identity-only previous version paired with its own hash (it matches, but it is not a whole document),
    and a whole document above the schema's `max_orders_per_day` paired with its own hash (it would classify as
    reducing; only the schema test refuses it).
    Every document built is checked schema-valid, so a refusal comes from the clause under test. The rest of the
    verdict is the same mandate validated without the policy."""
    lone_workspaces = {None, 0, 1}
    laxer = {"deny": "ask", "ask": "auto"}
    shapes = ["none", "identity", "reducing", "reducing", "reducing_default", "neutral", "neutral_quiet", "increasing",
              "mixed", "forged", "unhashed", "schema_invalid"]
    for _ in range(n):
        m = copy.deepcopy(base.BASES[rng.choice(sorted(base.BASES))])
        if rng.random() < 0.3:
            m["autonomy"]["approval"]["two_approver_above_usd"] = rng.choice(["500", "5000"])
        required, users = rng.random() < 0.5, rng.choice([None, 0, 1, 1, 2, 3, 5])
        ctx = {k: v for k, v in base.CTX.items() if k != "workspace_users"}
        ctx["disclosures_accepted"] = ["sha256:" + "b" * 64]
        if users is not None:
            ctx["workspace_users"] = users
        ctx["approver_users"] = min(rng.choice([1, 2]), 1 if users is None else users)
        if rng.random() < 0.9:
            ctx["independent_approval_required"] = required
        else:
            required = False
        shape = rng.choice(shapes)
        if shape == "reducing_default" and m["autonomy"]["default"] == "auto":
            m["autonomy"]["default"] = "ask"
        if shape == "identity":
            ctx["previous_version"] = {"environment": m["environment"], "connection_id": m["connection_id"]}
            ctx["current_mandate_version"] = version(ctx["previous_version"])
        elif shape != "none":
            prev = copy.deepcopy(m)
            orders = m["risk"]["max_orders_per_day"]
            if shape in ("reducing", "mixed", "forged", "unhashed"):
                prev["risk"]["max_orders_per_day"] = orders + 1
            if shape == "schema_invalid":
                prev["risk"]["max_orders_per_day"] = 10001
            if shape == "reducing_default":
                prev["autonomy"]["default"] = laxer[m["autonomy"]["default"]]
            if shape == "neutral":
                prev["name"] = m["name"] + "-before"
            if shape == "neutral_quiet":
                prev["notifications"]["quiet_hours"] = {"start": "21:30", "end": "06:15", "timezone": "America/New_York"}
            if shape == "increasing":
                m["risk"]["max_orders_per_day"] = orders + 1
            if shape == "mixed":
                prev["risk"]["max_order_usd"] = "1"
            check(V.is_valid(prev) != (shape == "schema_invalid") and prev != m,
                  "every previous version the fuzz builds is a whole document, schema-valid but for the one shape that "
                  "is not", (shape, prev))
            ctx["previous_version"] = prev
            if shape == "forged":
                real = copy.deepcopy(m)
                real["name"] = m["name"] + "-current"
                ctx["current_mandate_version"] = version(real)
            elif shape != "unhashed":
                ctx["current_mandate_version"] = version(prev)
        exempt = shape in ("reducing", "reducing_default")
        errs = semantic(m, ctx)[0]
        check(("V-047" in errs) == (required and users in lone_workspaces and not exempt),
              "V-047 refuses exactly independent approval in a workspace without a second user, an unknown count as one, "
              "save a risk-reducing version against the agent's current document",
              (required, users, shape, errs))
        without = semantic(m, dict(ctx, independent_approval_required=False))[0]
        check([e for e in errs if e != "V-047"] == without, "V-047 changes no other verdict",
              (required, users, shape, errs, without))

UNASKED_CANDIDATES = ["0.01", "1", "99.99", "100", "300", "450.5", "500", "899.99", "900", "950", "1000", "5000"]

def rand_order_cond():
    leaf = lambda: {"field": "order_usd", "op": rng.choice(["lt", "lte", "eq", "gt", "gte", "ne"]),
                    "value": rng.choice(["100", "300", "500", "900"])}
    other = lambda: {"field": "purpose", "op": "in", "value": ["increase", "open"]}
    shape = rng.choice(["leaf", "other", "all", "any", "not"])
    if shape == "leaf":
        return leaf()
    if shape == "other":
        return other()
    if shape == "not":
        return {"not": leaf()}
    return {shape: [rng.choice([leaf, other])() for _ in range(rng.randint(1, 3))]}

def rand_unasked_mandate():
    """A delegated mandate whose conditions also bound `order_usd` through `all`, `any`, and `not`, with a small
    daily order count, a gross limit that binds, and sometimes a review date already passed."""
    m = rand_delegated_mandate()
    if m is None:
        return None
    for x in m["autonomy"]["rules"] + m["autonomy"]["delegations"]:
        if rng.random() < 0.5:
            x["when"] = rand_order_cond()
    m["risk"]["max_orders_per_day"] = rng.randint(2, 8)
    m["risk"]["max_gross_exposure_usd"] = rng.choice(["2500", "4000.5", "10000"])
    rb = rng.choice([None, None, None, risk_day(fmt(DELEG_NOW))["risk_day"], "2026-09-21"])
    if rb is not None:
        m["autonomy"]["review_by"] = rb
    V.validate(m)
    return m

def rand_unasked_state(m):
    usage = {}
    for d in m["autonomy"]["delegations"]:
        if rng.random() < 0.5:
            usage[d["id"]] = {"orders": rng.randint(0, d["max_orders"]),
                              "total_usd": rng.choice(["0", "100", "899.5", d["max_total_usd"]])}
    return {"now": fmt(DELEG_NOW + timedelta(seconds=rng.choice([0, 600, 3600 * 9]))), "usage": usage,
            "agent_equity": rng.choice(["10000", "10000", "3000.1234", "800", "-5"]),
            "gross_usd": rng.choice(["0", "0", "250.0001", "1999.99", "12000"]), "orders_today": rng.randint(0, 3)}

def own_unasked_run(m, st0, pick, times):
    """The independent oracle: the real §6.2 decision (`autonomy`) over a day of opening orders, with its own
    accumulators for the gate's order size, order count, and gross exposure (§5.3) and for delegation usage. `pick`
    offers each order's values, given the gross headroom and usage left. Returns the order value decided `auto`."""
    r = m["risk"]
    left_orders = r["max_orders_per_day"] - st0["orders_today"]
    gross_left = min(D(r["max_gross_exposure_usd"]), D(st0["agent_equity"])) - D(st0["gross_usd"])
    usage = copy.deepcopy(st0["usage"])
    total = D(0)
    for t in times:
        if left_orders <= 0 or gross_left <= 0:
            break
        st = {"now": fmt(t), "usage": usage}
        for v in pick(gross_left, usage):
            a = rand_autonomy_action()
            a.update({"order_usd": norm(v), "new_instrument": False, "requested_by": "agent"})
            if v <= 0 or v > gross_left or v > D(r["max_order_usd"]):
                continue
            res = autonomy(m, a, st)
            if res["decision"] != "auto":
                continue
            total, gross_left, left_orders = total + v, gross_left - v, left_orders - 1
            did = res.get("delegation_id")
            if did is not None:
                u = usage.setdefault(did, {"orders": 0, "total_usd": "0"})
                u["orders"] += 1
                u["total_usd"] = norm(D(u["total_usd"]) + v)
            break
    return total

def fuzz_unasked(n):
    """§4.2's unasked dollars (DEC-189, DEC-695). Sound: no day of opening orders runs more unasked than the
    figure, whatever their values, times, and conditions. Unknown is never 0. Tight: where every condition is a
    catch-all or an `order_usd` bound and rules come before no ask, a greedy day reaches the figure exactly."""
    for _ in range(n):
        m = rand_unasked_mandate()
        if m is None:
            continue
        st = rand_unasked_state(m)
        fig = unasked_usd(m, st)
        gone = rng.choice(UNASKED_STATE)
        check(unasked_usd(m, {k: v for k, v in st.items() if k != gone}) is None,
              "unasked: a missing journal input is unknown, never 0", (st,))
        check(unasked_usd(m, st, {"auto_allowed": False}) == "0" and unasked_usd(m, st, {"nonconforming": True}) == "0",
              "unasked: a policy that allows no auto leaves nothing unasked", (st,))
        end = T(risk_day(st["now"])["ends_at"])
        span = int((end - T(st["now"])).total_seconds())
        for _ in range(3):
            times = sorted(T(st["now"]) + timedelta(seconds=rng.randint(0, span - 1)) for _ in range(12))
            ran = own_unasked_run(m, st, lambda g, _: [D(rng.choice(UNASKED_CANDIDATES + [norm(g)])) for _ in range(6)], times)
            check(ran <= D(fig), "unasked: no day runs more unasked than the figure", (m["autonomy"], st, fig, norm(ran)))
    for _ in range(n):
        m = copy.deepcopy(base.btc)
        au = m["autonomy"]
        lte = lambda: {"field": "order_usd", "op": "lte", "value": rng.choice(["300", "500", "900"])}
        bound = lambda: rng.choice([{"field": "purpose", "op": "in", "value": ["increase", "open"]}, lte(),
                                    {rng.choice(["all", "any"]): [lte(), lte()]}])
        if rng.random() < 0.5:
            au["rules"] = [{"id": f"r{i}", "when": bound(), "then": "auto"} for i in range(rng.randint(0, 2))]
            au["default"], lifts = "ask", "default"
        else:
            au["rules"] = [{"id": "r0", "when": bound(), "then": "ask"}]
            au["default"], lifts = "deny", "rule:r0"
        au["delegations"] = []
        for i in range(rng.randint(0, 3)):
            d = rand_delegation(i, {lifts})
            d["when"] = bound()
            d["starts_at"] = fmt(DELEG_NOW - timedelta(hours=1))
            d["expires_at"] = fmt(DELEG_NOW + timedelta(seconds=rng.choice([-60, 1, 86400])))
            au["delegations"].append(d)
        if rng.random() < 0.2:
            au["review_by"] = "2026-09-21"
        m["risk"]["max_orders_per_day"] = rng.randint(1, 6)
        m["risk"]["max_gross_exposure_usd"] = rng.choice(["2500", "4000.5", "10000"])
        V.validate(m)
        st = rand_unasked_state(m)
        st["now"] = fmt(DELEG_NOW)
        fig = unasked_usd(m, st)
        def greedy(g, usage):
            caps = {D("1000"), g} | {D(d["max_order_usd"]) for d in au["delegations"]}
            caps |= {D(c["value"]) for x in au["rules"] + au["delegations"] for c, _ in comparisons(x["when"])
                     if c["field"] == "order_usd"}
            for d in au["delegations"]:
                u = usage.get(d["id"], {"total_usd": "0"})
                caps.add(D(d["max_total_usd"]) - D(u["total_usd"]))
            return sorted({min(c, g) for c in caps if c > 0}, reverse=True)
        best = own_unasked_run(m, st, greedy, [DELEG_NOW] * 20)
        check(norm(best.quantize(D("0.01"), rounding=ROUND_CEILING)) == fig, "unasked: the figure is reached exactly",
              (au, st, fig, norm(best)))

def fuzz_loss_answer(n):
    """DEC-695 item 6: the loss answer's three fields are valid, never looser than the words, and keep V-014 and the
    floor above the drawdown above the daily loss. The oracle works in exact fractions."""
    for _ in range(n):
        a = rng.choice(["1000", "2500", "10000", "33333.33"])
        ans = rng.choice([("fraction", rng.choice(["0.1", "0.05", "0.12345", "0.00009", "0.9999", "1"])),
                          ("usd", rng.choice(["0", "1", "250", "999.99", "1000", "5000.55", "40000"]))])
        out = loss_answer_fields(ans, a)
        said = Fraction(ans[1]) / (Fraction(a) if ans[0] == "usd" else 1)
        own = Fraction(int(said * 10000), 10000)
        if not 0 < own < 1:
            check(out is None, "loss answer: no loss or the whole allocation is asked again", (ans, a, out))
            continue
        check(out is not None, "loss answer: a loss below the allocation maps", (ans, a))
        if out is None:
            continue
        floor, dd, day = (Fraction(out[p][1]) for p in ("/capital/max_loss_from_allocation", "/risk/max_drawdown",
                                                         "/risk/max_daily_loss"))
        check(floor == own and floor <= said and dd == own * Fraction(4, 5) and day == own / 5,
              "loss answer: the fields follow the stated loss, rounded down to basis points", (ans, a, out))
        m = copy.deepcopy(base.btc)
        m["capital"]["max_loss_from_allocation"], m["risk"]["max_drawdown"] = out["/capital/max_loss_from_allocation"][1], out["/risk/max_drawdown"][1]
        m["risk"]["max_daily_loss"] = out["/risk/max_daily_loss"][1]
        check(V.is_valid(m) and floor >= dd > day > 0, "loss answer: schema-valid and V-014", (ans, out))
        check([out[p][0] for p in sorted(out)] == ["user_stated", "platform_proposed", "platform_proposed"],
              "loss answer: only the floor is the owner's words", (out,))

if __name__ == "__main__":
    fuzz_ladder_precision(300)
    fuzz_risk(400)
    fuzz_stepped_lift(300)
    fuzz_gate(300)
    fuzz_builder(400)
    fuzz_admission(300)
    fuzz_expiry(400)
    fuzz_lineage(300)
    fuzz_pinning(400)
    fuzz_gate_universe(200)
    fuzz_autonomy(3000)
    fuzz_delegations(600)
    fuzz_delegation_changes(600)
    fuzz_delegation_rules(400)
    fuzz_delegated_rule_changes(1500)
    fuzz_client_ceiling(400)
    fuzz_review(600)
    fuzz_review_changes(600)
    fuzz_review_rules(600)
    fuzz_tripwires(400)
    fuzz_tripwire_changes(600)
    fuzz_tripwire_rules(600)
    fuzz_tripwire_latch(300)
    fuzz_escalation(3000)
    fuzz_policy_quorum(1000)
    fuzz_independence_floor(400)
    fuzz_drift(600)
    fuzz_ask_budget(600)
    fuzz_quiet_hours(600)
    fuzz_owner_controls(600)
    fuzz_content(200)
    fuzz_stop_limit_offset(600)
    fuzz_unasked(400)
    fuzz_loss_answer(400)
    print("failures:", len(FAIL), Counter(f[0] for f in FAIL))
    for name, ctx in FAIL[:3]:
        print("EXAMPLE", name, str(ctx)[:1500])
    sys.exit(1 if FAIL else 0)
