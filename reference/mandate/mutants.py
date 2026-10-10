"""Seeds known bugs into a copy of ref.py and checks that fuzz.py catches every one (AGENTS.md: independent oracles).

A bug that makes a base mandate invalid cannot be carried here. `bases.py` asserts at import that
every base passes `semantic()`, so such a probe crashes before any check runs, and `verdict()` scores
the crash `ERROR`, which is neither a catch nor a survival. V-047 has two such bugs, an inverted
policy and an ignored one: each fires V-047 on the bases. Do not add them, and do not read an
`ERROR` as a catch (#528 round 2, minor 4).
"""
import pathlib, shutil, subprocess, sys, tempfile

HERE = pathlib.Path(__file__).resolve().parent
REPO = HERE.parents[1]
MUTANTS = {
    "V-008 and W-002 read crypto, not the profile": (
        '    return any(c in classes for c in m["universe"]["asset_classes"])',
        '    return "crypto" in m["universe"]["asset_classes"]'),
    "a missing profile reads as the paper profile": (
        '    if classes is None:\n        return bool(m["universe"]["asset_classes"])',
        '    if classes is None:\n        classes = ["crypto"]'),
    "a missing profile needs no offset": (
        '    if classes is None:\n        return bool(m["universe"]["asset_classes"])',
        '    if classes is None:\n        return False'),
    "a version may move back to schema version 1": (
        '                             or m["mandate_schema_version"] < prev.get("mandate_schema_version", m["mandate_schema_version"])):',
        '                             or False):'),
    "V-008 ignores the profile": (
        '        if stop_limit_protected(m, ctx) and stop_limit_offset(p, m) is None:',
        '        if "crypto" in m["universe"]["asset_classes"] and stop_limit_offset(p, m) is None:'),
    "W-002 omits the stop-limit offset": (
        '    offset = D(stop_limit_offset(p, m) or 0) if stop_limit_protected(m, ctx) else D(0)', '    offset = D(0)'),
    "W-002 adds the offset without a stop-limit": (
        '    offset = D(stop_limit_offset(p, m) or 0) if stop_limit_protected(m, ctx) else D(0)',
        '    offset = D(stop_limit_offset(p, m) or 0)'),
    "a version-1 document is compared unread": (
        '    if old["mandate_schema_version"] != new["mandate_schema_version"]:', '    if False:'),
    "restart confirmation on every bounce": ("        if self.false_run >= need and not self.prev:\n            self.acc = 0.0",
                                             "        if not breached:\n            self.acc = 0.0"),
    "additive capital base": ("self.C, self.L = H1, E01, C1, L1", "self.C, self.L = H1, E01, self.C + d, L1"),
    "hard trigger latches on one quote": ("        if quote and (t - self.hard_first[key]).total_seconds() >= self.hard_wait():", "        if True:"),
    "V-047 reads a stated policy's presence, not its value": (
        '    if ctx.get("independent_approval_required", False) and ctx.get("workspace_users", 1) < 2:',
        '    if "independent_approval_required" in ctx and ctx.get("workspace_users", 1) < 2:'),
    "V-047 counts a one-user workspace as independent": (
        '    if ctx.get("independent_approval_required", False) and ctx.get("workspace_users", 1) < 2:',
        '    if ctx.get("independent_approval_required", False) and ctx.get("workspace_users", 1) < 1:'),
    "V-047 counts an unknown workspace as two users": (
        '    if ctx.get("independent_approval_required", False) and ctx.get("workspace_users", 1) < 2:',
        '    if ctx.get("independent_approval_required", False) and ctx.get("workspace_users", 2) < 2:'),
    "V-047 exempts a neutral version too": (
        '            and classify(prev, m)[0] == "risk_reducing")',
        '            and classify(prev, m)[0] in ("risk_reducing", "neutral"))'),
    "V-047 exempts every version of a running agent": (
        '            and classify(prev, m)[0] == "risk_reducing")',
        '            and True)'),
    "V-047 exempts a version when no current version is supplied": (
        '    return (prev is not None and current is not None and set(SCHEMA["required"]) <= set(prev)\n'
        '            and V.is_valid(prev) and version(prev) == current\n',
        '    return (prev is not None and set(SCHEMA["required"]) <= set(prev)\n'
        '            and V.is_valid(prev) and (current is None or version(prev) == current)\n'),
    "V-047 exempts a version whose previous document it does not have": (
        '    return (prev is not None and current is not None and set(SCHEMA["required"]) <= set(prev)\n'
        '            and V.is_valid(prev) and version(prev) == current\n'
        '            and classify(prev, m)[0] == "risk_reducing")',
        '    return (prev is not None and current is not None\n'
        '            and (not V.is_valid(prev)\n'
        '                 or (version(prev) == current and classify(prev, m)[0] == "risk_reducing")))'),
    "V-047 exempts a previous document the schema refuses": (
        '            and V.is_valid(prev) and version(prev) == current\n',
        '            and version(prev) == current\n'),
    "V-047 exempts against a document that is not the agent's current version": (
        '            and V.is_valid(prev) and version(prev) == current\n',
        '            and V.is_valid(prev)\n'),
    "loss carry ignores withdrawals": ("        self.net_contributed += d\n", ""),
    "release retires without a loss carry": ('                self._retire("goal_complete", ev)\n',
                                             '                self.restrictions["retired"] = "stopped"\n'),
    "release carries no loss": ('                self._retire("goal_complete", ev)\n',
                                '                self._retire("goal_complete", ev)\n                ev[-1]["loss_carry_usd"] = "0"\n'),
    "a re-triggered scale rung is held back during the stepped lift": (
        '                        if self.reset_queue:\n'
        '                            self.reset_queue = sorted(self.reset_queue + [i], key=lambda j: D(self.lad[j]["at"]), reverse=True)\n',
        '                        if i in self.reset_queue:\n                            self.reset_queue.remove(i)\n'),
    "a profit_stop goal completes as hold_protected": (
        '            ev.append({"type": "GoalCompleted", "then": "discretionary_exit_all_then_retire"})',
        '            ev.append({"type": "GoalCompleted", "on_complete": "hold_protected"})'),
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
    "a widened ask rule a delegation lifts is reducing": (
        '        if rb["then"] == "ask" and d > 0 and f"rule:{rb[\'id\']}" in lifted:\n            return "increasing"\n', ''),
    "removing a rule before a delegated ask is reducing": (
        '            if ra["then"] != "auto" and later_sources & lifted:\n                return "increasing"\n', ''),
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
                       'drafts.append(dict(approval_intent(st["pending"][a]), type="IntentProposed", approval=a, clock=c))'),
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
    "a response step judges before an exits-only mode cancels": ('        if inp["now"]["mode"] != "normal" and st["pending"]:',
                                                                 '        if False:'),
    "drift without the absolute value": ('abs(D(m_now) - D(m_req)) * 10000', '(D(m_now) - D(m_req)) * 10000'),
    "drift uses the crypto band for equities": ('<= DRIFT_BAND_BP[asset_class] * D(m_req)', '<= 200 * D(m_req)'),
    "no mark is inside the band": ('    if m_req is None or m_now is None:\n        return False\n    return abs(',
                                   '    if m_req is None or m_now is None:\n        return True\n    return abs('),
    "a grant re-prices at the current mark": (
        "    intent = approval_intent(req)",
        '    intent = approval_intent(req) | {"limit_price": now["mark"]}',
    ),
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
# Seeded bugs in the tripwires (§6.7, MI-31, V-044; DEC-187, DEC-350 to DEC-352). Each must be caught both by the
# tripwire fuzz (TW_PROBE) and by the MC-W reference cases, which `main` regenerates from the mutated model and requires
# to differ. TW_PROBE and the cases are first run on the unmutated model and must come back clean, so a catch is never
# a failure that was there before the mutation.
TRIPWIRE_MUTANTS = {
    "a tripwire fires one past its threshold": ('            if v >= D(t["threshold"]):', '            if v > D(t["threshold"]):'),
    "a fired tripwire fires again": ('            if tid in self.fired:\n                continue\n', ''),
    "an acknowledgment without valid step-up lifts a tripwire": ('            if verdict["result"] == "refused":\n                ev.append(',
                                                                 '            if False:\n                ev.append('),
    "a version that removes a fired tripwire lifts it": (
        '            self.counters = {k: v for k, v in self.counters.items() if k in self.current()}\n',
        '            self.counters = {k: v for k, v in self.counters.items() if k in self.current()}\n'
        '            self.fired = {k: v for k, v in self.fired.items() if k in self.current()}\n'),
    "tightening a tripwire re-arms it": ('if tid not in before or before[tid]["metric"] != t["metric"]:', 'if tid not in before or before[tid] != t:'),
    "changing a tripwire's metric keeps its count": ('if tid not in before or before[tid]["metric"] != t["metric"]:',
                                                     'if tid not in self.counters:'),
    "an acknowledgment does not re-arm the tripwire": ('                if tid in self.current():\n                    self.counters[tid] = self.armed()\n', ''),
    "the realized loss carries across risk days": ('            for c in self.counters.values():\n                c["day_net"] = D(0)\n',
                                                   '            pass\n'),
    "a break-even exit extends the losing streak": ('c["streak"] + 1 if net < 0 else 0', 'c["streak"] + 1 if net <= 0 else 0'),
    "a buy breaks the losing streak": ('                if inp["side"] == "sell":\n                    c["streak"] = c["streak"] + 1 if net < 0 else 0\n',
                                       '                c["streak"] = (c["streak"] + 1 if net < 0 else 0) if inp["side"] == "sell" else 0\n'),
    "a buy's fees are not realized loss": ('                c["day_net"] += net\n', '                c["day_net"] += net if inp["side"] == "sell" else 0\n'),
    "a fill's own fees are left out": ('    return gross - D(f["fees"])', '    return gross'),
    "the removed basis rounds half-up": ('            u = b - r12(b * qty / q)', '            u = b - (b * qty / q).quantize(D("1e-12"), rounding="ROUND_HALF_UP")'),
    "new_instruments counts a re-entry": ('            first = inp["instrument"] not in self.ever_filled\n',
                                          '            first = inp["side"] == "buy" and self.book[inp["instrument"]][0] == D(inp["qty"])\n'),
    "a fired tripwire leaves delegations lifting": ('\n            or st.get("tripwire_fired", False))', ')'),
    "an exits_only tripwire pauses the agent": ('"restriction": "exits_only" if "exits_only" in held.values() else None',
                                                '"restriction": "paused" if "exits_only" in held.values() else None'),
    "a softened action loosens a fired tripwire": ('if now is not None and TRIPWIRE_ACTIONS[now["action"]] > TRIPWIRE_ACTIONS[a]:', 'if now is not None:'),
    "the alert names the tripwire": ('"text": TRIPWIRE_ALERT_TEXT})', '"text": f"{TRIPWIRE_ALERT_TEXT} {tid}"})'),
    "removing a tripwire is reducing": ('        if (b is None or b["metric"] != a["metric"]', '        if b is None:\n            continue\n        if (b["metric"] != a["metric"]'),
    "raising a threshold is reducing": (' or D(b["threshold"]) > D(a["threshold"])', ''),
    "softening an action is reducing": ('\n                or TRIPWIRE_ACTIONS[b["action"]] < TRIPWIRE_ACTIONS[a["action"]]):', '):'),
    "changing a metric is reducing": ('b["metric"] != a["metric"] or ', ''),
    "V-044 allows a fractional count": ('ok = th == th.to_integral_value() and 1 <= th', 'ok = 1 <= th'),
    "V-044 allows a count above 1,000": ('1 <= th <= TRIPWIRE_MAX_COUNT', '1 <= th'),
    "V-044 allows a loss above the allocation": (' and th <= D(m["capital"]["allocation_usd"])', ''),
    "V-044 allows a fraction of a cent": ('ok = -th.as_tuple().exponent <= 2 and ', 'ok = '),
    "midnight resets the losing streak": ('            for c in self.counters.values():\n                c["day_net"] = D(0)\n',
                                          '            for c in self.counters.values():\n                c["day_net"] = D(0)\n                c["streak"] = 0\n'),
    "a refused acknowledgment does not spend its assertion": (
        '            if inp.get("step_up") is not None and isinstance(inp["step_up"], dict) and "assertion" in inp["step_up"]:',
        '            if verdict["result"] == "apply" and isinstance(inp.get("step_up"), dict) and "assertion" in inp["step_up"]:'),
    "the requester lifts a tripwire under independent approval": (
        '            elif ((inp.get("independent_approval_required", False) or inp.get("independent_now", False))\n'
        '                  and (inp.get("user") is None or inp.get("requester") is None or inp.get("user") == inp.get("requester"))):\n',
        '            elif False:\n'),
    "a fired tripwire does not latch the risk state": ('        for tid in held["fired"]:\n            self.latched[f"tripwire:{tid}"] = True\n', ''),
    "only an exits_only tripwire latches the risk state": ('        for tid in held["fired"]:\n            self.latched[f"tripwire:{tid}"] = True\n',
                                                           '        for tid in held["fired"]:\n            if held["fired"][tid] == "exits_only":\n                self.latched[f"tripwire:{tid}"] = True\n'),
    "the risk state does not pass RiskDayStarted to the tripwires": ('            if self.tw is not None:\n                self._tw_input = {"event": "RiskDayStarted"}\n', ''),
    "an acknowledgment naming no requester is independent": (
        'and (inp.get("user") is None or inp.get("requester") is None or inp.get("user") == inp.get("requester"))):',
        'and inp.get("user") == inp.get("requester")):'),
    "independence is read only at processing": ('(inp.get("independent_approval_required", False) or inp.get("independent_now", False))',
                                                'inp.get("independent_now", False)'),
    "V-044 allows unsorted ids": ('    if not sorted_unique([t["id"] for t in tws]):\n        return {"V-044"}', '    if False:\n        return {"V-044"}'),
}
# The trim's minimum (§5.5, DEC-399 item 5) is judged by the family-B cases rather than by a fuzz:
# MC-B33 and MC-B34 sit where the instrument's minimum order size and the dollar minimum disagree,
# MC-B33 also sits on the boundary, a trim exactly at the minimum, MC-B35 is the full close the
# minimum exempts (DEC-423), MC-B36 and MC-B37 size the trim after a resting sell (DEC-399 item 7),
# and MC-B38 and MC-B39 carry DEC-445 item 2: the resting sells come off the excess before it is
# rounded up on the grid, and an off-grid remainder beside them is truncated onto it.
TRIM_MUTANTS = {
    "the trim's minimum is the dollar minimum order": ('        if sell < D(inp["min_order_size"]) and sell != qty:',
                                                      '        if sell * bid < D(inp["min_order_usd"]) and sell != qty:'),
    "the trim's minimum is ignored": ('        if sell < D(inp["min_order_size"]) and sell != qty:', '        if False:'),
    "a full close is withheld below the minimum": ('        if sell < D(inp["min_order_size"]) and sell != qty:',
                                                   '        if sell < D(inp["min_order_size"]):'),
    "a trim at the minimum is withheld": ('        if sell < D(inp["min_order_size"]) and sell != qty:',
                                          '        if sell <= D(inp["min_order_size"]) and sell != qty:'),
    "a trim ignores the agent's resting sells": ('''        owed = max(D(0), mv - factor * cap - on_sale * bid)\n        rounded = ceil_inc(owed / bid, inc)\n        unsold = max(D(0), qty - on_sale)''',
                                                 '''        owed = max(D(0), mv - factor * cap)\n        rounded = ceil_inc(owed / bid, inc)\n        unsold = qty'''),
    "the trim rounds up before it takes the resting sells off the excess": (
        '        owed = max(D(0), mv - factor * cap - on_sale * bid)\n        rounded = ceil_inc(owed / bid, inc)',
        '        owed = mv - factor * cap\n        rounded = max(D(0), ceil_inc(owed / bid, inc) - on_sale)'),
    "an off-grid remainder beside a resting sell is rounded up past what is unsold": (
        '            sell = trunc(unsold, inc)',
        '            sell = ceil_inc(unsold, inc)'),
}

UNASKED_MUTANTS = {
    "unasked: unknown reads as 0": (
        '    if st is None or any(k not in st for k in UNASKED_STATE):\n        return None',
        '    if st is None or any(k not in st for k in UNASKED_STATE):\n        return "0"'),
    "unasked: the review date is ignored": (
        '    if review_passed(m, st) or policy.get', '    if policy.get'),
    "unasked: a policy forbidding auto is ignored": (
        ' or not policy.get("auto_allowed", True):', ':'),
    "unasked: today's orders are not counted": (
        '    n = max(0, r["max_orders_per_day"] - st["orders_today"])', '    n = r["max_orders_per_day"]'),
    "unasked: delegation usage is not counted": (
        '        k = max(0, d["max_orders"] - used["orders"])', '        k = d["max_orders"]'),
    "unasked: a delegation's spent total is not counted": (
        '        rest = max(D(0), D(d["max_total_usd"]) - D(used["total_usd"]))', '        rest = D(d["max_total_usd"])'),
    "unasked: a delegation's last partial order is dropped": (
        '        if full < k and rest - full * c > 0:', '        if False:'),
    "unasked: an expired delegation still counts": (
        '        if not (d["starts_at"] < day_end and st["now"] < d["expires_at"]):', '        if False:'),
    "unasked: a delegation's expiry is truncated to the second (DEC-903)": (
        '        if not (d["starts_at"] < day_end and st["now"] < d["expires_at"]):',
        '        if not (d["starts_at"] < day_end and T(st["now"]) < T(d["expires_at"])):'),
    "unasked: a delegation starting at the end of the risk day counts (DEC-903)": (
        '        if not (d["starts_at"] < day_end and st["now"] < d["expires_at"]):',
        '        if not (d["starts_at"] <= day_end and st["now"] < d["expires_at"]):'),
    "unasked: a delegation's condition bound is ignored": (
        '        c = capped(per_order, D(d["max_order_usd"]), order_usd_bound(d["when"]),',
        '        c = capped(per_order, D(d["max_order_usd"]), None,'),
    "unasked: the lifted rule's bound is ignored": (
        '                   order_usd_bound(rules[d["lifts"]]) if d["lifts"] in rules else None)', '                   None)'),
    "unasked: a rule's condition bound is ignored": (
        '    auto_caps = [capped(per_order, order_usd_bound(x["when"]))', '    auto_caps = [capped(per_order, None)'),
    "unasked: an any-condition takes its tightest member": (
        '        return max(bs) if bs and None not in bs else None', '        return min(bs) if bs and None not in bs else None'),
    "unasked: a not-condition bounds order_usd": (
        '    if "all" in c:\n        bs = [b for b', '    if "not" in c:\n        return order_usd_bound(c["not"])\n    if "all" in c:\n        bs = [b for b'),
    "unasked: the smallest slices are taken first": (
        'key=lambda s: s[0], reverse=True)', 'key=lambda s: s[0])'),
    "unasked: the old cap at the gross headroom at t (#1062 review B1)": (
        '    return norm(total.quantize(D("0.01"), rounding=ROUND_CEILING))',
        '    return norm(min(total, max(D(0), min(D(r["max_gross_exposure_usd"]), D(st["agent_equity"])) - D(st["gross_usd"])))'
        '.quantize(D("0.01"), rounding=ROUND_CEILING))'),
    "unasked: the figure rounds down": (
        '    return norm(total.quantize(D("0.01"), rounding=ROUND_CEILING))',
        '    return norm(total.quantize(D("0.01"), rounding=ROUND_DOWN))'),
    "loss answer: rounds up": (
        '    f = f.quantize(D("0.0001"), rounding=ROUND_DOWN)', '    f = f.quantize(D("0.0001"), rounding=ROUND_UP)'),
    "loss answer: the whole allocation maps": (
        '    if not D(0) < f < D(1) or proposed_ladder', '    if not D(0) < f <= D(1) or proposed_ladder'),
    "loss answer: the drawdown is the floor": ('norm(f * D("0.8"))', 'norm(f)'),
    "loss answer: the old mapping, the base ladder kept (#1062 review M1)": (
        '    dd = D(max_drawdown)\n', '    dd = D("0.08")\n'),
    "loss answer: the ladder rounds up": (
        '    bp = lambda x: (x * dd).quantize(D("0.0001"), rounding=ROUND_DOWN)',
        '    bp = lambda x: (x * dd).quantize(D("0.0001"), rounding=ROUND_UP)'),
    "loss answer: collapsed rungs are proposed": (
        '    if not D(0) < hyst < halve < exits < dd < D(1):', '    if not D(0) < dd < D(1):'),
    "loss answer: an allocation of 0 or less drafts or raises (DEC-901)": (
        '    if D(allocation_usd) <= 0:\n        return None\n', ''),
    "loss answer: a drawdown of 1 or more is proposed (DEC-901)": (
        '    if not D(0) < hyst < halve < exits < dd < D(1):', '    if not D(0) < hyst < halve < exits < dd:'),
}

PROBE = ("import sys; sys.argv=['x','1']; exec(open('fuzz.py').read().split('if __name__')[0]); "
         "fuzz_ladder_precision(200); fuzz_risk(400); fuzz_stepped_lift(300); fuzz_gate(200); fuzz_gate_universe(200); fuzz_admission(300); fuzz_expiry(400); "
         "fuzz_lineage(300); fuzz_pinning(400); fuzz_autonomy(1500); "
         "fuzz_delegations(400); fuzz_delegation_changes(400); fuzz_delegation_rules(300); fuzz_delegated_rule_changes(2500); fuzz_client_ceiling(300); "
         "fuzz_review(400); fuzz_review_changes(400); fuzz_review_rules(400); "
         "fuzz_escalation(1500); fuzz_policy_quorum(500); fuzz_independence_floor(300); fuzz_drift(300); fuzz_ask_budget(600); fuzz_quiet_hours(400); fuzz_owner_controls(600); fuzz_content(200); fuzz_stop_limit_offset(300); "
         "print(len(FAIL))")

UNASKED_PROBE = ("import sys; sys.argv=['x','1']; exec(open('fuzz.py').read().split('if __name__')[0]); "
                 "fuzz_unasked(600); fuzz_loss_answer(400); print(len(FAIL))")

TW_PROBE = ("import sys; sys.argv=['x','1']; exec(open('fuzz.py').read().split('if __name__')[0]); "
            "fuzz_tripwires(300); fuzz_tripwire_changes(400); fuzz_tripwire_rules(300); fuzz_tripwire_latch(200); print(len(FAIL))")

CASE_PROBE = ("import json, yaml\n"
              "try:\n    import generate\nexcept AssertionError:\n    print('differ')\n    raise SystemExit(0)\n"
              "mine = [c for c in generate.doc['cases'] if c['id'].startswith('MC-W')]; "
              "kept = [c for c in yaml.safe_load(open('../../docs/specs/reference-cases/mandate.yaml'))['cases'] if c['id'].startswith('MC-W')]; "
              "print('differ' if json.loads(json.dumps(mine)) != kept else 'same')")

B_CASE_PROBE = ("import json, yaml\n"
                "try:\n    import generate\nexcept AssertionError:\n    print('differ')\n    raise SystemExit(0)\n"
                "mine = [c for c in generate.doc['cases'] if c['id'].startswith('MC-B')]; "
                "kept = [c for c in yaml.safe_load(open('../../docs/specs/reference-cases/mandate.yaml'))['cases'] if c['id'].startswith('MC-B')]; "
                "print('differ' if json.loads(json.dumps(mine)) != kept else 'same')")

def run(work, probe):
    return subprocess.run([sys.executable, "-c", probe], cwd=work, capture_output=True, text=True, timeout=900)

def verdict(out, clean):
    """A probe's last line, or ERROR when it crashed or printed nothing: a crash is neither a catch nor a survival."""
    lines = out.stdout.strip().splitlines()
    if out.returncode != 0 or not lines:
        return "ERROR"
    return "caught" if lines[-1] != clean else "missed"

def main():
    bad = []
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        (root / "schemas").symlink_to(REPO / "schemas")
        (root / "docs").symlink_to(REPO / "docs")
        work = root / "reference" / "mandate"
        shutil.copytree(HERE, work, ignore=shutil.ignore_patterns("__pycache__"))
        source = (HERE / "ref.py").read_text()
        missing = [name for name, (old, _) in (MUTANTS | TRIPWIRE_MUTANTS | TRIM_MUTANTS | UNASKED_MUTANTS).items() if old not in source]
        assert not missing, f"mutation anchors missing, checked before any run: {missing}"
        assert verdict(run(work, PROBE), "0") == "missed", "the shared fuzz fails on the unmutated model"
        assert verdict(run(work, TW_PROBE), "0") == "missed", "the tripwire fuzz fails on the unmutated model"
        assert verdict(run(work, UNASKED_PROBE), "0") == "missed", "the unasked-dollars fuzz fails on the unmutated model"
        assert verdict(run(work, CASE_PROBE), "same") == "missed", "the MC-W cases differ from the unmutated model's"
        assert verdict(run(work, B_CASE_PROBE), "same") == "missed", "the MC-B cases differ from the unmutated model's"
        for name, (old, new) in (MUTANTS | TRIPWIRE_MUTANTS | TRIM_MUTANTS | UNASKED_MUTANTS).items():
            shutil.rmtree(work, ignore_errors=True)
            shutil.copytree(HERE, work, ignore=shutil.ignore_patterns("__pycache__"))
            ref = work / "ref.py"
            text = ref.read_text()
            assert old in text, f"mutation anchor missing: {name}"
            ref.write_text(text.replace(old, new, 1))
            if name in UNASKED_MUTANTS:
                status = {"caught": "caught", "missed": "SURVIVED", "ERROR": "ERROR"}[verdict(run(work, UNASKED_PROBE), "0")]
            elif name in TRIM_MUTANTS:
                status = {"caught": "caught", "missed": "SURVIVED", "ERROR": "ERROR"}[verdict(run(work, B_CASE_PROBE), "same")]
                name = f"{name} (MC-B cases)"
            elif name in TRIPWIRE_MUTANTS:
                by_fuzz, by_cases = verdict(run(work, TW_PROBE), "0"), verdict(run(work, CASE_PROBE), "same")
                status = "ERROR" if "ERROR" in (by_fuzz, by_cases) else ("caught" if by_fuzz == by_cases == "caught" else "SURVIVED")
                name = f"{name} (fuzz {by_fuzz}, MC-W cases {by_cases})"
            else:
                status = {"caught": "caught", "missed": "SURVIVED", "ERROR": "ERROR"}[verdict(run(work, PROBE), "0")]
            print(f"{status:8} {name}", flush=True)
            if status != "caught":
                bad.append(name)
    print(f"{len(MUTANTS) + len(TRIPWIRE_MUTANTS) + len(TRIM_MUTANTS) + len(UNASKED_MUTANTS)} mutants, {len(bad)} not caught", flush=True)
    sys.exit(1 if bad else 0)

if __name__ == "__main__":
    main()
