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
}
PROBE = ("import sys; sys.argv=['x','1']; exec(open('fuzz.py').read().split('if __name__')[0]); "
         "fuzz_risk(400); fuzz_gate(200); print(len(FAIL))")

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
