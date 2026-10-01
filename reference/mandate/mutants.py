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
    "the approval content drops the client": ('"client": req.get("client") if req.get("requested_by") == "client" else None',
                                               '"client": None'),
    "the approval content shows every requester as the agent": ('"requested_by": req.get("requested_by", "agent"),', '"requested_by": "agent",'),
    "the approval content hides the delegation shapes": ('["approve", "skip"] + list(req.get("delegation_shapes", []))', '["approve", "skip"]'),
    "a timeout acts": ('                drafts.append({"type": "ApprovalTimedOut", "approval": a, "on_timeout": "skip", "clock": c})',
                       '                drafts.append({"type": "ApprovalTimedOut", "approval": a, "on_timeout": "skip", "clock": c}); '
                       'drafts.append(dict({k: st["pending"][a][k] for k in BOUND_FIELDS}, type="IntentProposed", approval=a, clock=c))'),
    "lateness uses the submitted time alone": ('    eff = max(T(resp["submitted_at"]), T(ctx["clock"]))', '    eff = T(resp["submitted_at"])'),
    "a response exactly at the deadline is timely": ('    if eff >= T(req["deadline"]):', '    if eff > T(req["deadline"]):'),
    "the fold keeps an acted approval pending": ('    elif t in ("ApprovalRevalidated", "ApprovalTimedOut", "ApprovalCanceled"):',
                                                 '    elif t in ("ApprovalTimedOut", "ApprovalCanceled"):'),
    "a re-tailed response is judged again": ('        if resp["source"] in st["copied"]:\n            return drafts',
                                             '        if False:\n            return drafts'),
    "a version change does not cancel": ('        for a in sorted(st["pending"]):\n            drafts.append({"type": "ApprovalCanceled"',
                                         '        for a in (sorted(st["pending"]) if inp["reason"] != "version_applied" else []):\n'
                                         '            drafts.append({"type": "ApprovalCanceled"'),
    "re-validation overrides a deny": ('    if c["decision"] == "deny":\n        return skip("reclassified_deny")',
                                       '    if False:\n        return skip("reclassified_deny")'),
    "re-validation accepts an ask by another trigger": ('    if c["decision"] == "ask" and c["by"] != req["decided_by"]:', '    if False:'),
    "re-validation skips the gate dry run": ('    if now["dry_run"]["verdict"] != "allow":', '    if False:'),
    "re-validation ignores the mode": ('    if now["mode"] != "normal":\n        return skip("mode")', '    if False:\n        return skip("mode")'),
    "drift without the absolute value": ('abs(D(m_now) - D(m_req)) * 10000', '(D(m_now) - D(m_req)) * 10000'),
    "drift uses the crypto band for equities": ('<= DRIFT_BAND_BP[asset_class] * D(m_req)', '<= 200 * D(m_req)'),
    "no mark is inside the band": ('    if m_req is None or m_now is None:\n        return False\n    return abs(',
                                   '    if m_req is None or m_now is None:\n        return True\n    return abs('),
    "a grant re-prices at the current mark": ('"intent": {k: req[k] for k in BOUND_FIELDS}}',
                                              '"intent": {k: req[k] for k in BOUND_FIELDS} | {"limit_price": now["mark"]}}'),
    "the notification carries the instrument": ('return {"subject": req["approval"], "text": "approval_needed"}',
                                                'return {"subject": req["approval"], "text": "approval_needed " + req["instrument"]}'),
    "an agent actor is admitted": ('    if resp["actor_kind"] != "user" or resp["responder"] not in ctx["approvers"]:',
                                   '    if resp["responder"] not in ctx["approvers"]:'),
    "a responder outside approvers is admitted": ('    if resp["actor_kind"] != "user" or resp["responder"] not in ctx["approvers"]:',
                                                  '    if resp["actor_kind"] != "user":'),
    "step-up freshness is exclusive": ('    if not 0 <= age <= STEP_UP_WINDOW_S:', '    if not 0 <= age < STEP_UP_WINDOW_S:'),
    "step-up accepts evidence from the future": ('    if not 0 <= age <= STEP_UP_WINDOW_S:', '    if not age <= STEP_UP_WINDOW_S:'),
    "step-up is judged at the submitted time": ('evidence_fault(resp["step_up"], fmt(eff), ', 'evidence_fault(resp["step_up"], resp["submitted_at"], '),
    "assertion reuse accepted": ('    if ev["assertion"] in used:', '    if False:'),
    "CliConfirm accepted for live": ('    if not (ev["method"] == "cli_confirm" and environment == "paper"):', '    if not ev["method"] == "cli_confirm":'),
    "the content hash is not compared": ('    if resp["content_hash"] != req["content_hash"]:', '    if False:'),
    "the same approver counts twice": ('    if resp["responder"] in req["grants"]:', '    if False:'),
    "a counted grant acts": ('    return judged("admitted" if len(counting) + 1 >= q["required"] else "counted")', '    return judged("admitted")'),
    "independence is not enforced": ('    if q["independent"] and resp["responder"] == ctx["author"]:', '    if False:'),
    "check 7 reads only the bound approver requirement": (
        '    return {"required": max(req["approvers_required"], by_policy),\n'
        '            "independent": req["independent_required"] or policy["independent_approval_required"]}',
        '    return {"required": req["approvers_required"], "independent": req["independent_required"]}'),
    "check 7 takes the looser of the bound and policy requirement": (
        '    return {"required": max(req["approvers_required"], by_policy),\n'
        '            "independent": req["independent_required"] or policy["independent_approval_required"]}',
        '    return {"required": min(req["approvers_required"], by_policy),\n'
        '            "independent": req["independent_required"] and policy["independent_approval_required"]}'),
    "a lowered policy ceiling does not raise the approver count": (
        '    by_policy = 2 if ceiling is not None and D(req["qty"]) * D(req["limit_price"]) > D(ceiling) else 1',
        '    by_policy = 1'),
    "the author's earlier grant counts toward an independent quorum": (
        '    counting = {g for g in req["grants"] if not (q["independent"] and g == ctx["author"])}',
        '    counting = req["grants"]'),
    "ApprovalResponded drops the quorum check 7 applied": (
        '    judged = lambda result, reason=None: out(result, reason) | {"quorum": q}',
        '    judged = lambda result, reason=None: out(result, reason)'),
    "ApprovalResponded records the bound quorum, not the one check 7 applied": (
        '    judged = lambda result, reason=None: out(result, reason) | {"quorum": q}',
        '    judged = lambda result, reason=None: out(result, reason) | '
        '{"quorum": {"required": req["approvers_required"], "independent": req["independent_required"]}}'),
    "an undelivered request is grantable": ('    if not req["delivered"]:', '    if False:'),
    "admission reads the pending set before the batch's cancellations": (
        '            out = escalation_step(after, {"kind": "response"', '            out = escalation_step(st, {"kind": "response"'),
    "a pending approval holds an exit": ('    if purpose in REDUCING:\n        return "handed"\n    return "awaiting_approval" if pending else "evaluate"',
                                         '    if pending:\n        return "awaiting_approval"\n    return "handed" if purpose in REDUCING else "evaluate"'),
    "a second approval is asked while one is pending": ('    return "awaiting_approval" if pending else "evaluate"', '    return "evaluate"'),
    "pause demands step-up": ('    if kind == "pause":\n        return {"result": "apply", "reason": None}', '    if False:\n        return {"result": "apply", "reason": None}'),
    "an owner exit is judged when processed": ('    at = committed_at if kind == "owner_exit" else processed_at', '    at = processed_at'),
    "resume is judged when committed": ('    at = committed_at if kind == "owner_exit" else processed_at', '    at = committed_at'),
    "a refused owner exit is dropped": ('    privilege = confirmed_bid and authority["result"] == "apply"\n',
                                        '    privilege = confirmed_bid and authority["result"] == "apply"\n'
                                        '    if authority["result"] != "apply":\n        return {"verdict": "refused", "reason": authority["reason"]}\n'),
    "a refused owner exit keeps its privilege": ('    privilege = confirmed_bid and authority["result"] == "apply"\n', '    privilege = confirmed_bid\n'),
    "a kill switch without valid step-up keeps the privilege": ('owner_confirmed_bid=inp.get("owner_confirmed_bid", False) and fault is None',
                                                               'owner_confirmed_bid=inp.get("owner_confirmed_bid", False)'),
    "a kill switch without valid step-up is refused": ('    fault = evidence_fault(ev, committed_at, environment, used)\n    out = agent_flatten(',
                                                       '    fault = evidence_fault(ev, committed_at, environment, used)\n'
                                                       '    if fault is not None:\n        return {"mode_applied_first": None, "sells": [], "deferred_sells": [], "step_up": fault}\n'
                                                       '    out = agent_flatten('),
    "the ask budget resets at UTC midnight": ('    today = risk_day(at)["risk_day"]\n    asked, skipped, timed_out = 0, False, False\n    for e in ledger:\n'
                                              '        if e["event"] == "requested" and risk_day(e["at"])["risk_day"] == today:',
                                              '    today = at[:10]\n    asked, skipped, timed_out = 0, False, False\n    for e in ledger:\n'
                                              '        if e["event"] == "requested" and e["at"][:10] == today:'),
    "the ask budget allows an eleventh": ('    if asked >= ASK_BUDGET_PER_RISK_DAY:', '    if asked > ASK_BUDGET_PER_RISK_DAY:'),
    "a version does not lift a skip": ('        elif e["event"] == "version_applied":\n            skipped = False',
                                       '        elif e["event"] == "version_applied":\n            pass'),
    "the timeout window includes its end": ('(T(at) - T(e["at"])).total_seconds() < e["timeout_s"]:', '(T(at) - T(e["at"])).total_seconds() <= e["timeout_s"]:'),
    "quiet hours in UTC": ('    local = T(at).astimezone(NY)\n    minute = local.hour', '    local = T(at)\n    minute = local.hour'),
    "quiet hours suppress the inbox": ('    if channel == "cli_inbox" or quiet_hours is None:', '    if quiet_hours is None:'),
    "quiet hours include their end": ('% 1440 < (end - start) % 1440', '% 1440 <= (end - start) % 1440'),
    "the trigger drops the owner's rule": ('"rule": None if rule is None else {"id": rule["id"], "when": rule["when"], "then": rule["then"]}', '"rule": None'),
    "the review ceiling's trigger names a rule": ('    rule = rules.get(by[len("rule:"):]) if by.startswith("rule:") else None\n    return {"mandate_version"',
                                                  '    rule = rules.get(by[len("rule:"):]) if by.startswith("rule:") else (next(iter(rules.values()), None) if by == "review_ceiling" else None)\n    return {"mandate_version"'),
    "the review ceiling is skipped": ('    if review_passed(m, st) and STRICT[res["decision"]] < STRICT["ask"]:',
                                      '    if False:'),
    "the review date passes on the date itself": ('    return risk_day(st["now"])["risk_day"] > rb', '    return risk_day(st["now"])["risk_day"] >= rb'),
    "the review date passes at UTC midnight": ('    return risk_day(st["now"])["risk_day"] > rb', '    return st["now"][:10] > rb'),
    "no risk clock reads as before the review date": ('    if st is None or "now" not in st:\n        return True',
                                                      '    if st is None or "now" not in st:\n        return False'),
    "the review ceiling turns a deny into ask": ('    if review_passed(m, st) and STRICT[res["decision"]] < STRICT["ask"]:',
                                                 '    if review_passed(m, st):'),
    "the review ceiling relabels an ask": ('    if review_passed(m, st) and STRICT[res["decision"]] < STRICT["ask"]:',
                                           '    if review_passed(m, st) and STRICT[res["decision"]] <= STRICT["ask"]:'),
    "a delegation still lifts past the review date": (
        '    if review_passed(m, st) and STRICT[res["decision"]] < STRICT["ask"]:',
        '    if review_passed(m, st) and STRICT[res["decision"]] < STRICT["ask"] and "delegation_id" not in res:'),
    "the review date holds exits": ('        return {"decision": "auto", "by": "builtin_risk_reducing"}\n    res = None',
                                    '        return {"decision": "ask" if review_passed(m, st) else "auto", "by": "builtin_risk_reducing"}\n    res = None'),
    "a later review date is reducing": (
        '        elif p == "/autonomy/review_by":\n            res.add("increasing" if b is None or (a is not None and b > a) else "reducing")',
        '        elif p == "/autonomy/review_by":\n            res.add("increasing" if b is None else "reducing")'),
    "removing the review date is reducing": (
        '        elif p == "/autonomy/review_by":\n            res.add("increasing" if b is None or (a is not None and b > a) else "reducing")',
        '        elif p == "/autonomy/review_by":\n            res.add("increasing" if b is not None and a is not None and b > a else "reducing")'),
    "V-046 re-checks a carried date": ('    if rb is None or rb == prev_rb:\n        return errs', '    if rb is None:\n        return errs'),
    "V-046 lets a set date be removed": ('    if prev_rb is not None and rb is None:\n        errs.add("V-046")', '    if False:\n        errs.add("V-046")'),
    "V-046 allows 181 days": ('    if not vd <= rb <= days_after(vd, REVIEW_MAX_DAYS):', '    if not vd <= rb <= days_after(vd, REVIEW_MAX_DAYS + 1):'),
    "V-046 allows a date before validation": ('    if not vd <= rb <= days_after(vd, REVIEW_MAX_DAYS):', '    if not rb <= days_after(vd, REVIEW_MAX_DAYS):'),
    "a platform-default review date may be any date": (
        '                if m["autonomy"].get("review_by") != days_after(ctx["validation_date"], REVIEW_DEFAULT_DAYS):', '                if False:'),
    "re-confirming drops the delegations": ('classify(without_delegations(prev, review_by_of=m), without_delegations(m))',
                                            'classify(without_delegations(prev), without_delegations(m))'),
    "a bound field does not move the content hash": ('content_hash({k: req[k] for k in sorted(req)})', 'content_hash({k: req[k] for k in sorted(req) if k != "limit_price"})'),
}
PROBE = ("import sys; sys.argv=['x','1']; exec(open('fuzz.py').read().split('if __name__')[0]); "
         "fuzz_ladder_precision(200); fuzz_risk(400); fuzz_gate(200); fuzz_gate_universe(200); fuzz_admission(300); fuzz_expiry(400); "
         "fuzz_lineage(300); fuzz_pinning(400); fuzz_autonomy(1500); "
         "fuzz_delegations(400); fuzz_delegation_changes(400); fuzz_delegation_rules(300); fuzz_client_ceiling(300); "
         "fuzz_review(400); fuzz_review_changes(400); fuzz_review_rules(400); "
         "fuzz_escalation(1500); fuzz_policy_quorum(500); fuzz_drift(300); fuzz_ask_budget(600); fuzz_quiet_hours(400); fuzz_owner_controls(600); fuzz_content(200); "
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
