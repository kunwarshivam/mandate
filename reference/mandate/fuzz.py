"""Property-based fuzz of the v0.3 reference implementation against invariants MI-1 to MI-11."""
import copy, random, sys
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
        else:
            q = D(10 if cls == "us_equity" else "0.01")
            qty += q
            steps.append({"event": "fill", "at": at, "session": sess, "side": "buy", "qty": norm(q), "price": norm(px.quantize(D("0.01")))})
    return steps

def run(m, cls, steps, start, L="0"):
    rs = RiskState(m, "100" if cls == "us_equity" else "0.1", "100" if cls == "us_equity" else "60000", cls, start, inherited_loss=L)
    outs = []
    for s in steps:
        before = copy.deepcopy((rs.restrictions, {k: v["active"] for k, v in rs.scale.items()}, dict(rs.latched), rs.daily))
        pre = (rs.equity(), rs.H, rs.E0, rs.C)
        o = rs.step(s)
        outs.append(o)
        types = [e["type"] for e in o["journal"]]
        # MI-2
        if s["event"] == "allocation_change" and "error" not in o:
            after = types[types.index("MandateVersionApplied") + 1:]
            check(not any(x in ("RiskLimitTriggered", "RiskLimitLifted", "KillSwitchActivated", "AgentModeApplied") for x in after),
                  "MI-2 no trigger or lift on allocation change", (s, o))
            E, H, E0, C = pre
            E1 = D(o["agent_equity"])
            check((D(o["high_water_mark"]) - E1) / D(o["high_water_mark"]) >= (H - E) / H, "MI-2 drawdown not lowered", (s, o))
            check((E1 - D(o["capital_base"])) / D(o["capital_base"]) <= (E - C) / C, "MI-2 return not raised", (s, o))
        # MI-3
        removed = set(before[0]) - set(rs.restrictions)
        for rname in removed:
            if rname in ("drawdown_exits_only", "drawdown_flatten"):
                check(s["event"] == "owner_acknowledged" and s["restriction"] == "drawdown_ladder", "MI-3 drawdown latch lifted by ack only", (s, o))
            if rname == "lifetime_floor":
                check(False, "MI-3 floor never lifts", (s, o))
            if rname == "daily_loss":
                check(before[3] is not None and before[3]["day_started"] or s["event"] == "risk_day_started", "MI-3 daily lifts on a new day", (s, o))
        # MI-4 hard floor
        E, C = D(o["agent_equity"]), D(o["capital_base"])
        if E <= C * (1 - D("1.25") * rs.f) + rs.L:
            check("lifetime_floor" in rs.restrictions, "MI-4 hard floor latched", (s, o))
        # MI-5
        H = D(o["high_water_mark"])
        check(H >= E and D(0) <= (H - E) / H < 1, "MI-5 H >= E", (s, o))
        # MI-6
        strictest = max(rs.restrictions.values(), key=lambda x: SEVERITY[x], default="normal")
        check(o["agent_mode"] == strictest, "MI-6 mode is strictest", (s, o))
        changed = before[0] != rs.restrictions and max(before[0].values(), key=lambda x: SEVERITY[x], default="normal") != strictest
        check(("AgentModeApplied" in types) == changed, "MI-6 AgentModeApplied iff change", (s, o))
        # MI-7
        if s["event"] == "allocation_change" and D(s["delta_usd"]) > 0 and (before[2] or before[3] is not None):
            check(o.get("error") == "increase_blocked_while_latched", "MI-7 increase blocked while latched", (s, o))
    return outs

def fuzz_risk(n_runs):
    for k in range(n_runs):
        cls = rng.choice(["us_equity", "crypto"])
        m = rand_mandate(cls)
        start = "2026-09-21T13:35:00.000000000Z"
        steps = rand_steps(cls, rng.randint(20, 80), start)
        L = rng.choice(["0", "0", "300"])
        a = run(m, cls, steps, start, L)
        b = run(m, cls, steps, start, L)
        check(a == b, "MI-8 replay identical", k)
        # MI-13: dropping clock ticks that emitted no events changes no other output
        keep = [i for i, st in enumerate(steps) if not (st["event"] == "clock" and not a[i]["journal"])]
        c = run(m, cls, [steps[i] for i in keep], start, L)
        strip = lambda o: {x: y for x, y in o.items() if x != "pending"}
        check([strip(a[i]) for i in keep] == [strip(x) for x in c], "MI-13 no-event ticks are replay-safe", k)
        # MI-4 confirmed floor: continuous breach for >= need seconds implies latch
        rs = RiskState(m, "100" if cls == "us_equity" else "0.1", "100" if cls == "us_equity" else "60000", cls, start, inherited_loss=L)
        since = None
        for s in steps:
            o = rs.step(s)
            E, C = D(o["agent_equity"]), D(o["capital_base"])
            breached = E <= C * (1 - rs.f) + rs.L
            t = T(s["at"])
            if breached:
                since = since or t
                if (t - since).total_seconds() >= rs.need:
                    check("lifetime_floor" in rs.restrictions, "MI-4 confirmed floor latched", (s, o))
            else:
                since = None

def fuzz_gate(n):
    m = copy.deepcopy(base.swing)
    for _ in range(n):
        st = {"now": "2026-09-22T15:00:00.000000000Z", "agent_equity": norm(D(rng.randint(5000, 12000))),
              "positions_mv": {base.XYZ: norm(D(rng.randint(0, 3000)))}, "working_opening_orders": [],
              "orders_today": rng.randint(0, 60), "last_exit_fill_at": {base.XYZ: "2026-09-22T14:59:00.000000000Z"}}
        for purpose in ("risk_exit", "protective", "owner_exit", "discretionary_exit"):
            g = gate(m, st, {"instrument": base.XYZ, "purpose": purpose, "qty": str(rng.randint(1, 500)), "limit_price": "100"})
            check(g["verdict"] == "allow", "MI-1 exits allowed by mandate limits", (purpose, st))

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
                              "orders_today": 0, "last_exit_fill_at": {}},
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

def fuzz_autonomy(n):
    m = copy.deepcopy(base.btc)
    for _ in range(n):
        m["autonomy"]["rules"] = [rand_rule(i) for i in range(rng.randint(0, 4))]
        m["autonomy"]["default"] = rng.choice(["ask", "deny", "auto"])
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
                     "position_usd_after": "0", "gross_usd_after": "0"}
                for f, vals in FIELDS_NUM.items():
                    a[f] = rng.choice(vals + [norm(D(v) + D("0.01")) for v in vals])
                d0, d1 = autonomy(m, a)["decision"], autonomy(new, a)["decision"]
                check(STRICT[d1] >= STRICT[d0], "MI-11 reducing change never loosens autonomy",
                      (m["autonomy"], new["autonomy"], a, d0, d1))

if __name__ == "__main__":
    fuzz_risk(400)
    fuzz_gate(300)
    fuzz_builder(400)
    fuzz_autonomy(3000)
    from collections import Counter
    print("failures:", len(FAIL), Counter(f[0] for f in FAIL))
    for name, ctx in FAIL[:3]:
        print("EXAMPLE", name, str(ctx)[:1500])
