"""Seeds known bugs into a copy of ref.py and checks that fuzz.py catches every one (AGENTS.md: independent oracles)."""
import pathlib, shutil, subprocess, sys, tempfile

HERE = pathlib.Path(__file__).resolve().parent
REPO = HERE.parents[1]
MUTANTS = {
    "restart confirmation on every bounce": ("        if self.false_run >= need and not self.prev:\n            self.acc = 0.0",
                                             "        if not breached:\n            self.acc = 0.0"),
    "additive capital base": ("self.C, self.L = H1, E01, C1, L1", "self.C, self.L = H1, E01, self.C + d, L1"),
    "hard trigger latches on one quote": ("        if quote and (t - self.hard_first[key]).total_seconds() >= self.hard_wait():", "        if True:"),
    "loss carry ignores withdrawals": ("        self.net_contributed += d\n", ""),
    "allocation increase allowed while latched": ("        if d > 0 and (self.latched or self.daily is not None):", "        if False:"),
    "daily lift ignores the minimum delay": ('(t - self.daily["at"]).total_seconds() >= self.daily_min', "True"),
    "flatten acknowledged while not flat": ('            if "drawdown_flatten" in self.restrictions and self.qty > 0:\n'
                                            '                return "flatten_in_progress"\n', ""),
    "admission ignores max_instruments": ('yield "universe_full", not renewal and len(active) >= u["max_instruments"]',
                                          'yield "universe_full", False'),
    "the pinned mode admits": ('    yield "research_disabled", not admitting or res is None\n'
                               '    yield "universe_pinned", u["pinned"]\n',
                               '    yield "research_disabled", False\n    yield "universe_pinned", False\n'),
    "admission ignores the eligibility floor": ('yield "eligibility_floor", inst in inp.get("eligibility_failures", [])',
                                                'yield "eligibility_floor", False'),
    "admission ignores corroboration": ('yield "no_corroboration", not (th["corroboration"] or {}).get("kind")',
                                        'yield "no_corroboration", False'),
    "admission ignores the source allowlist": ('yield "source_not_allowlisted", any(s not in inp.get("allowlisted_sources", []) for s in th["evidence_sources"])',
                                               'yield "source_not_allowlisted", False'),
    "admission ignores the owner's admission ceiling": (
        '    if a.get("new_instrument", False) and STRICT[au["admission"]] > STRICT[res["decision"]]:',
        '    if False:'),
    "lineage cap is off by one": ('yield "lineage_retired", lineage.get("retired", False) or th["revision"] > cap',
                                  'yield "lineage_retired", lineage.get("retired", False) or th["revision"] > cap + 1'),
    "expiry keeps an invalidated thesis": ('        if e.get("invalidated"):\n            why = "thesis_invalidated"\n',
                                           '        if False:\n            why = "thesis_invalidated"\n'),
    "expiry ignores the horizon": ('        elif now >= T(e["expires_at"]):', '        elif False:'),
    "expiry ignores a retired lineage": ('        elif lineages.get(e["lineage_id"], {}).get("retired", False):',
                                         '        elif False:'),
    "the gate fails open without a working universe": ('    if inst not in st["working_universe"]:',
                                                       '    if inst not in st.get("working_universe", [inst]):'),
    "pinning is reducing without a research agent": (
        '            had_agent = any(s["admits_instruments"] for s in old["behavior"]["signal_models"])\n'
        '            res.add("reducing" if b and had_agent else "increasing")',
        '            res.add("reducing" if b else "increasing")'),
    "a retired lineage keeps its instrument": ('            if held in universe:\n'
                                              '                universe = [i for i in universe if i != held]\n',
                                              '            if False:\n'
                                              '                universe = [i for i in universe if i != held]\n'),
    "retirement follows the revision number, not the refusal reason": (
        '        if r["reason"] == "lineage_retired" and not st["retired"]:',
        '        if th["revision"] > cap and not st["retired"]:'),
    "retirement removes what another lineage holds": (
        '            for lid in [k for k, v in holders.items() if v == th["instrument_id"] and k != th["lineage_id"]]:\n'
        '                del holders[lid]\n', ""),
    "V-040 is never raised": ('        errs.add("V-040")', '        pass'),
    "V-040 bounds only the whole product": (
        'if sum(-D(x["factor"]).as_tuple().exponent for x in lad if x["action"] == "scale_sizes" and x["factor"] is not None) > 12:',
        'if not (lambda p: p == p.quantize(D("1e-12")))(__import__("math").prod([D(x["factor"]) for x in lad if x["action"] == "scale_sizes"], start=D(1))):'),
    "admission ignores the leveraged-ETP disclosure": (
        '        u["leveraged_etps_enabled"] and u["leveraged_etp_disclosure_version"] in inp.get("disclosures_accepted", []))',
        '        u["leveraged_etps_enabled"])'),
    "a delegation lifts a deny": ('if st is None or res["decision"] != "ask" or', 'if st is None or res["decision"] == "auto" or'),
    "a delegation ignores the ask it names": ('if (d["lifts"] == res["by"] and T(d["starts_at"])', 'if (T(d["starts_at"])'),
    "a delegation ignores its condition": ('                and cond(d["when"], a) and D(a["order_usd"])', '                and D(a["order_usd"])'),
    "a delegation ignores max_order_usd": (' and D(a["order_usd"]) <= D(d["max_order_usd"])', ''),
    "a delegation ignores max_orders": ('used["orders"] < d["max_orders"]', 'True'),
    "a delegation ignores max_total_usd": ('D(used["total_usd"]) + D(a["order_usd"]) <= D(d["max_total_usd"])', 'True'),
    "a delegation ignores its expiry": ('now < T(d["expires_at"])', 'True'),
    "a delegation ignores its start": ('T(d["starts_at"]) <= now', 'True'),
    "a delegation ignores suspension": (' or delegation_suspended(st):', ':'),
    "a delegation ignores a latched limit": ('\n            or st.get("limit_latched", False)', '\n            or False'),
    "a delegation bypasses the admission ceiling": (
        '    if a.get("new_instrument", False) and STRICT[au["admission"]] > STRICT[res["decision"]]:',
        '    if a.get("new_instrument", False) and lifted_by is None and STRICT[au["admission"]] > STRICT[res["decision"]]:'),
    "adding a delegation is reducing": ('        if i not in oi:\n            return "increasing"', '        if i not in oi:\n            continue'),
    "raising a delegation's cap is reducing": ('all(D(b[k]) <= D(a[k]) for k in DELEGATION_CAPS)', 'True'),
    "extending a delegation's expiry is reducing": (' and T(b["expires_at"]) <= T(a["expires_at"])', ''),
    "V-041 accepts a span past 30 days": ('not 0 < span <= DELEGATION_MAX_SPAN_S', 'not 0 < span'),
    "V-042 is never raised": ('            errs.add("V-042")', '            pass'),
    "V-043 ignores the second approver": ('(two is None or D(d["max_order_usd"]) <= D(two))', 'True'),
    "the client ceiling is skipped": ('a.get("requested_by", "agent") == "client" and ', 'False and '),
    "the client ceiling turns a deny into an ask": (
        'a.get("requested_by", "agent") == "client" and STRICT[res["decision"]] < STRICT["ask"]',
        'a.get("requested_by", "agent") == "client"'),
    "a delegation lifts a client request": (
        '    lifted_by = delegation_lift(m, a, res, st)',
        '    lifted_by = delegation_lift(m, a, res, st)\n'
        '    if lifted_by is not None and a.get("requested_by") == "client":\n'
        '        return {"decision": "auto", "by": f"delegation:{lifted_by}", "delegation_id": lifted_by}'),
    "the client ceiling reaches owner requests": ('a.get("requested_by", "agent") == "client"', 'a.get("requested_by", "agent") != "agent"'),
}
PROBE = ("import sys; sys.argv=['x','1']; exec(open('fuzz.py').read().split('if __name__')[0]); "
         "fuzz_ladder_precision(200); fuzz_risk(400); fuzz_gate(200); fuzz_gate_universe(200); fuzz_admission(300); fuzz_expiry(400); "
         "fuzz_lineage(300); fuzz_pinning(400); fuzz_autonomy(1500); "
         "fuzz_delegations(400); fuzz_delegation_changes(400); fuzz_delegation_rules(300); fuzz_client_ceiling(300); "
         "print(len(FAIL))")

def main():
    survivors = []
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        (root / "schemas").symlink_to(REPO / "schemas")
        (root / "docs").symlink_to(REPO / "docs")
        work = root / "reference" / "mandate"
        for name, (old, new) in MUTANTS.items():
            shutil.rmtree(work, ignore_errors=True)
            shutil.copytree(HERE, work, ignore=shutil.ignore_patterns("__pycache__"))
            ref = work / "ref.py"
            text = ref.read_text()
            assert old in text, f"mutation anchor missing: {name}"
            ref.write_text(text.replace(old, new, 1))
            out = subprocess.run([sys.executable, "-c", PROBE], cwd=work, capture_output=True, text=True, timeout=900)
            caught = out.returncode == 0 and int(out.stdout.strip().splitlines()[-1]) > 0
            print(f"{'caught  ' if caught else 'SURVIVED'} {name}")
            if not caught:
                survivors.append(name)
    sys.exit(1 if survivors else 0)

if __name__ == "__main__":
    main()
