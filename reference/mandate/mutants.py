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
    "admission ignores the leveraged-ETP disclosure": (
        '        u["leveraged_etps_enabled"] and u["leveraged_etp_disclosure_version"] in inp.get("disclosures_accepted", []))',
        '        u["leveraged_etps_enabled"])'),
}
PROBE = ("import sys; sys.argv=['x','1']; exec(open('fuzz.py').read().split('if __name__')[0]); "
         "fuzz_risk(400); fuzz_gate(200); fuzz_gate_universe(200); fuzz_admission(300); fuzz_expiry(400); "
         "fuzz_lineage(300); fuzz_pinning(400); fuzz_autonomy(1500); "
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
