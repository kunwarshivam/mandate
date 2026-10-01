"""Generates docs/specs/reference-cases/mandate.yaml for mandate spec v0.6."""
import copy, json
from collections import Counter
import yaml
from ref import *  # noqa: F401,F403
from ref import D, ROOT
from bases import *  # noqa: F401,F403

doc = {"version": 4, "spec": "docs/specs/mandate.md (v0.6)",
       "schemas": ["schemas/mandate.schema.json", "schemas/policy.schema.json"],
       "harness_defaults": {"mark_max_age_s": 120, "hard_trigger_multiple": "1.25", "stagger_window_s": 900},
       "bases": {n: {"mandate": m, "canonical_sha256": version(m)} for n, m in BASES.items()},
       "version_vector": {"base": "btc_accumulator", "canonical": canon(btc), "mandate_version": version(btc)},
       "signal_model_registry": REGISTRY, "validation_context_defaults": CTX, "cases": []}
cases = doc["cases"]
MB = dict(BASES)

def rep(path, value):
    return {"op": "replace", "path": path, "value": value}

def derived(name, base, patch, note):
    m = apply_patch(MB[base], patch)
    V.validate(m)
    MB[name] = m
    doc["bases"][name] = {"mandate": m, "canonical_sha256": version(m), "derived_from": base, "note": note}

NO_PROT = {"enabled": False, "stop_distance": None, "take_profit_distance": None, "crypto_stop_limit_offset": None}
RESEARCH = {"interval_s": 3600, "cost_cap_usd_per_day": "5", "max_revisions_per_lineage": 3}

# =========================================================== A. schema
S = [
    ("MC-S01", "Base mandates are schema-valid", "btc_accumulator", [], True),
    ("MC-S02", "on_timeout other than skip", "btc_accumulator", [rep("/autonomy/approval/on_timeout", "execute")], False),
    ("MC-S03", "Fraction outside (0, 1)", "btc_accumulator", [rep("/risk/max_drawdown", "8")], False),
    ("MC-S04", "Non-canonical decimal", "btc_accumulator", [rep("/risk/max_daily_loss", "0.020")], False),
    ("MC-S05", "Unknown top-level field", "btc_accumulator", [{"op": "add", "path": "/leverage", "value": "2"}], False),
    ("MC-S06", "Decimal as JSON number", "btc_accumulator", [rep("/capital/allocation_usd", 10000)], False),
    ("MC-S07", "Unknown condition field", "btc_accumulator",
     [rep("/autonomy/rules/0/when", {"field": "vibes", "op": "eq", "value": True})], False),
    ("MC-S08", "Signal model id without type prefix", "btc_accumulator", [rep("/behavior/signal_models/0/id", "mean_reversion")], False),
    ("MC-S09", "Cadence faster than 60 s", "btc_accumulator", [rep("/behavior/cadence/interval_s", 30)], False),
    ("MC-S10", "Goal missing a required parameter", "btc_accumulator", [{"op": "remove", "path": "/goal/max_spend_usd"}], False),
    ("MC-S11", "Removed goal type distribute", "btc_accumulator",
     [rep("/goal", {"type": "distribute", "instrument": BTC, "sell_qty": "0.1", "min_avg_price": None, "end_date": None})], False),
    ("MC-S12", "Signal model weight 0", "btc_accumulator", [rep("/behavior/signal_models/0/weight", "0")], False),
    ("MC-S13", "scale_sizes factor 1 (no effect)", "btc_accumulator", [rep("/risk/drawdown_ladder/0/factor", "1")], False),
    ("MC-S14", "Hysteresis 0", "btc_accumulator", [rep("/risk/hysteresis", "0")], False),
    ("MC-S15", "Protection enabled without a stop distance", "btc_accumulator", [rep("/protection/stop_distance", None)], False),
    ("MC-S16", "Integer signal-model parameter", "btc_accumulator", [rep("/behavior/signal_models/0/params/0/value", 20)], False),
    ("MC-S17", "Decimal with 29 fractional digits", "btc_accumulator", [rep("/risk/max_daily_loss", "0." + "0" * 27 + "11")], False),
    ("MC-S18", "Month 13", "btc_accumulator", [rep("/goal/end_date", "2026-13-01")], False),
    ("MC-S19", "Empty in-list", "btc_accumulator", [rep("/autonomy/rules/2/when/value", [])], False),
    ("MC-S20", "accumulate without on_complete", "btc_accumulator", [{"op": "remove", "path": "/goal/on_complete"}], False),
    ("MC-S21", "breach_confirm_s above the 300 s platform maximum", "btc_accumulator", [rep("/risk/breach_confirm_s", 301)], False),
    ("MC-S22", "Signal model without max_output_age_s", "btc_accumulator",
     [{"op": "remove", "path": "/behavior/signal_models/0/max_output_age_s"}], False),
    ("MC-S23", "Unknown scale_action", "btc_accumulator", [rep("/risk/scale_action", "sell_everything")], False),
    ("MC-S24", "max_instruments above the platform ceiling of 20", "research_equity", [rep("/universe/max_instruments", 21)], False),
    ("MC-S25", "max_instruments of 0", "research_equity", [rep("/universe/max_instruments", 0)], False),
    ("MC-S26", "Empty asset_classes", "research_equity", [rep("/universe/asset_classes", [])], False),
    ("MC-S27", "Research object missing the cost cap", "research_equity",
     [{"op": "remove", "path": "/behavior/research/cost_cap_usd_per_day"}], False),
    ("MC-S28", "Unknown autonomy.admission value", "research_equity", [rep("/autonomy/admission", "sometimes")], False),
    ("MC-S29", "Signal model without admits_instruments", "btc_accumulator",
     [{"op": "remove", "path": "/behavior/signal_models/0/admits_instruments"}], False),
    ("MC-S30", "max_revisions_per_lineage above 10", "research_equity", [rep("/behavior/research/max_revisions_per_lineage", 11)], False),
    ("MC-S31", "Research interval below 300 s", "research_equity", [rep("/behavior/research/interval_s", 60)], False),
]
for cid, title, base, patch, ok in S:
    m = apply_patch(MB[base], patch)
    assert V.is_valid(m) == ok, (cid, [e.message for e in V.iter_errors(m)])
    cases.append({"id": cid, "kind": "schema", "title": title, "base": base, "patch": patch, "expect": {"schema_valid": ok}})

# =========================================================== B. semantic
PU = lambda s, c=True: {"source": s, "confirmed": c}
SEM = [
    ("MC-V01", "Base accumulator passes every V-rule (warnings only)", "btc_accumulator", [], {}),
    ("MC-V02", "Base swing mandate passes every V-rule", "two_stock_swing", [], {}),
    ("MC-V03", "Allocations exceed account equity", "btc_accumulator", [], {"other_allocations_usd": "16000"}),
    ("MC-V04", "Allocations exactly equal account equity", "btc_accumulator", [], {"other_allocations_usd": "15000"}),
    ("MC-V05", "Accumulate universe must be pinned to exactly the goal instrument", "btc_accumulator",
     [rep("/universe/pinned", False), rep("/universe/pinned_instruments", [])], {}),
    ("MC-V06", "Leveraged ETPs without an accepted disclosure", "two_stock_swing",
     [rep("/universe/leveraged_etps_enabled", True), rep("/universe/leveraged_etp_disclosure_version", "sha256:" + "b" * 64)],
     {"disclosures_accepted": ["sha256:" + "c" * 64]}),
    ("MC-V07", "Leveraged ETPs with the accepted disclosure version", "two_stock_swing",
     [rep("/universe/leveraged_etps_enabled", True), rep("/universe/leveraged_etp_disclosure_version", "sha256:" + "b" * 64)],
     {"disclosures_accepted": ["sha256:" + "b" * 64]}),
    ("MC-V08", "Instrument in a group claimed by another agent", "two_stock_swing", [],
     {"instrument_groups": {QRS: "grp_qrs", LMN: "grp_qrs"}, "claimed_by_other_agents": [LMN]}),
    ("MC-V09", "Signal model content hash differs from the registry", "btc_accumulator",
     [rep("/behavior/signal_models/0/content_hash", "sha256:" + "9" * 64)], {}),
    ("MC-V10", "Missing declared parameter", "btc_accumulator", [{"op": "remove", "path": "/behavior/signal_models/0/params/1"}], {}),
    ("MC-V11", "Crypto with protection but no stop-limit offset", "btc_accumulator", [rep("/protection/crypto_stop_limit_offset", None)], {}),
    ("MC-V12", "Protection disabled but distances set", "btc_accumulator", [rep("/protection/enabled", False)], {}),
    ("MC-V13", "Protection disabled with distances null (warning W-003)", "btc_accumulator", [rep("/protection", NO_PROT)], {}),
    ("MC-V14", "Instruments not sorted by asset_id", "two_stock_swing",
     [rep("/universe/pinned_instruments", [{"asset_id": QRS, "symbol": "QRS", "asset_class": "us_equity"},
                                          {"asset_id": XYZ, "symbol": "XYZ", "asset_class": "us_equity"}])], {}),
    ("MC-V15", "Duplicate rule id", "btc_accumulator", [rep("/autonomy/rules/1/id", "large_orders")], {}),
    ("MC-V16", "Ladder rungs not strictly increasing", "btc_accumulator", [rep("/risk/drawdown_ladder/1/at", "0.03")], {}),
    ("MC-V17", "Ladder actions out of severity order", "btc_accumulator",
     [rep("/risk/drawdown_ladder/0", {"at": "0.03", "action": "exits_only", "factor": None}),
      rep("/risk/drawdown_ladder/1", {"at": "0.06", "action": "scale_sizes", "factor": "0.5"})], {}),
    ("MC-V18", "Last rung not flatten_and_pause at max_drawdown", "btc_accumulator", [rep("/risk/max_drawdown", "0.09")], {}),
    ("MC-V19", "Hysteresis not below the first rung", "btc_accumulator", [rep("/risk/hysteresis", "0.03")], {}),
    ("MC-V20", "Order limit above position limit", "two_stock_swing", [rep("/risk/max_order_usd", "2000")], {}),
    ("MC-V21", "Lifetime loss floor below max_drawdown", "btc_accumulator", [rep("/capital/max_loss_from_allocation", "0.05")], {}),
    ("MC-V22", "Invalid calendar date", "btc_accumulator", [rep("/goal/end_date", "2026-02-30")], {}),
    ("MC-V23", "Quiet hours start equals end", "btc_accumulator", [rep("/notifications/quiet_hours/end", "23:00")], {}),
    ("MC-V24", "Condition nested deeper than 4", "btc_accumulator",
     [rep("/autonomy/rules/0/when", {"all": [{"any": [{"not": {"all": [{"field": "order_usd", "op": "gt", "value": "900"}]}}]}]})], {}),
    ("MC-V25", "unusual_input is not available in v1", "btc_accumulator",
     [{"op": "add", "path": "/autonomy/rules/0", "value": {"id": "unusual", "when": {"field": "unusual_input", "op": "eq", "value": True}, "then": "ask"}}], {}),
    ("MC-V26", "Judgment field from a platform default", "btc_accumulator", [], {"provenance": {"/risk/max_daily_loss": PU("platform_default")}}),
    ("MC-V27", "Judgment field stated by the user but not confirmed", "btc_accumulator", [],
     {"provenance": {"/risk/max_order_usd": PU("user_stated", False)}}),
    ("MC-V28", "Platform defaults on listed non-envelope fields (single-user workspace)", "btc_accumulator", [],
     {"provenance": {"/notifications/channels": PU("platform_default"), "/autonomy/default": PU("platform_default"),
                     "/autonomy/approval/approvers": PU("platform_default")}}),
    ("MC-V29", "Platform-default approvers in a multi-user workspace", "btc_accumulator", [],
     {"workspace_users": 3, "provenance": {"/autonomy/approval/approvers": PU("platform_default")}}),
    ("MC-V30", "Platform default for autonomy.default must be ask", "btc_accumulator", [rep("/autonomy/default", "deny")],
     {"provenance": {"/autonomy/default": PU("platform_default")}}),
    ("MC-V49", "Platform default environment must be paper", "btc_accumulator", [rep("/environment", "live")],
     {"connection_environment": "live", "provenance": {"/environment": PU("platform_default")}}),
    ("MC-V50", "Connection is never a platform default", "btc_accumulator", [],
     {"provenance": {"/connection_id": PU("platform_default")}}),
    ("MC-V51", "trim_to_target is invalid with accumulate", "btc_accumulator", [rep("/risk/scale_action", "trim_to_target")], {}),
    ("MC-V31", "auto rule extracted by the compiler (not user-entered)", "btc_accumulator", [],
     {"provenance": {"/autonomy/rules/2": PU("user_stated")}}),
    ("MC-V32", "Default auto that is user-entered and confirmed", "btc_accumulator", [rep("/autonomy/default", "auto")],
     {"provenance": {"/autonomy/default": PU("user_entered")}}),
    ("MC-V33", "Purpose value outside open/increase", "btc_accumulator", [rep("/autonomy/rules/2/when/value", ["discretionary_exit", "open"])], {}),
    ("MC-V34", "Purpose ne with a reducing value", "btc_accumulator",
     [{"op": "add", "path": "/autonomy/rules/0",
       "value": {"id": "sneaky", "when": {"not": {"field": "purpose", "op": "ne", "value": "risk_exit"}}, "then": "ask"}}], {}),
    ("MC-V35", "Comparison operator with a non-decimal value", "btc_accumulator", [rep("/autonomy/rules/0/when/value", "lots")], {}),
    ("MC-V36", "in operator with a scalar value", "btc_accumulator", [rep("/autonomy/rules/2/when/value", "open")], {}),
    ("MC-V37", "Score threshold outside [0, 1]", "btc_accumulator", [rep("/autonomy/rules/1/when/value", "1.5")], {}),
    ("MC-V38", "Non-canonical decimal in a condition", "btc_accumulator", [rep("/autonomy/rules/0/when/value", "900.0")], {}),
    ("MC-V39", "Two-approver threshold with one approver", "btc_accumulator", [rep("/autonomy/approval/two_approver_above_usd", "700")], {}),
    ("MC-V40", "Two-approver threshold with two approvers", "btc_accumulator", [rep("/autonomy/approval/two_approver_above_usd", "700")],
     {"approver_users": 2}),
    ("MC-V41", "End date before the validation date", "btc_accumulator", [rep("/goal/end_date", "2026-09-23")], {}),
    ("MC-V42", "End date equal to the validation date", "btc_accumulator", [rep("/goal/end_date", "2026-09-24")], {}),
    ("MC-V43", "Environment changed from the previous version", "btc_accumulator", [rep("/environment", "live")],
     {"previous_version": {"environment": "paper", "connection_id": "conn_alpaca_paper_01"}, "connection_environment": "live"}),
    ("MC-V44", "Connection loss carry at or above the floor budget blocks deployment", "btc_accumulator", [],
     {"connection_loss_carry_usd": "1000"}),
    ("MC-V45", "Connection loss carry below the floor budget", "btc_accumulator", [], {"connection_loss_carry_usd": "300"}),
    ("MC-V46", "Eligibility floor failure is a warning", "two_stock_swing", [], {"eligibility_failures": [QRS]}),
    ("MC-V47", "A rule after a catch-all rule is shadowed (warning)", "btc_accumulator",
     [{"op": "add", "path": "/autonomy/rules/-", "value": {"id": "never_reached", "when": {"field": "order_usd", "op": "gt", "value": "5000"}, "then": "deny"}}], {}),
    ("MC-V48", "Multiple violations are all reported", "two_stock_swing",
     [rep("/risk/max_order_usd", "2000"), rep("/risk/hysteresis", "0.05")], {"other_allocations_usd": "20000"}),
    ("MC-V52", "Base research mandate passes every V-rule", "research_equity", [], {}),
    ("MC-V53", "Not pinned but pinned instruments are set", "research_equity",
     [rep("/universe/pinned_instruments", [{"asset_id": XYZ, "symbol": "XYZ", "asset_class": "us_equity"}])], {}),
    ("MC-V54", "Pinned with an empty pinned universe", "two_stock_swing", [rep("/universe/pinned_instruments", [])], {}),
    ("MC-V55", "max_instruments below the pinned count", "two_stock_swing", [rep("/universe/max_instruments", 1)], {}),
    ("MC-V56", "Two signal models admitting instruments", "research_equity",
     [rep("/behavior/signal_models/1/admits_instruments", True)], {}),
    ("MC-V57", "A quant model may not admit instruments", "research_equity",
     [rep("/behavior/signal_models/0/admits_instruments", False), rep("/behavior/signal_models/1/admits_instruments", True)], {}),
    ("MC-V58", "Research fields set with no admitting model", "two_stock_swing", [rep("/behavior/research", RESEARCH)], {}),
    ("MC-V59", "Pinning the universe disables the research agent", "research_equity",
     [rep("/universe/pinned", True),
      rep("/universe/pinned_instruments", [{"asset_id": XYZ, "symbol": "XYZ", "asset_class": "us_equity"}])], {}),
    ("MC-V60", "Accumulate with research fields set", "btc_accumulator", [rep("/behavior/research", RESEARCH)], {}),
    ("MC-V61", "Pinned instrument outside the envelope asset classes", "two_stock_swing",
     [rep("/universe/asset_classes", ["crypto"]), rep("/protection/crypto_stop_limit_offset", "0.005")], {}),
    ("MC-V62", "Pinned instruments proposed by the platform", "two_stock_swing", [],
     {"provenance": {"/universe/pinned_instruments": PU("platform_proposed")}}),
    ("MC-V63", "Platform default for autonomy.admission must be ask", "research_equity", [rep("/autonomy/admission", "deny")],
     {"provenance": {"/autonomy/admission": PU("platform_default")}}),
    ("MC-V64", "Admission auto extracted by the compiler, not entered", "research_equity", [rep("/autonomy/admission", "auto")],
     {"provenance": {"/autonomy/admission": PU("user_stated")}}),
    ("MC-V65", "Admission auto that is user-entered and confirmed is a warning", "research_equity",
     [rep("/autonomy/admission", "auto")], {"provenance": {"/autonomy/admission": PU("user_entered")}}),
    ("MC-V66", "Envelope field proposed by the platform and confirmed is fine", "research_equity", [],
     {"provenance": {"/universe/max_instruments": PU("platform_proposed"), "/risk/max_daily_loss": PU("platform_proposed")}}),
    ("MC-V67", "Platform-proposed envelope field left unconfirmed", "research_equity", [],
     {"provenance": {"/universe/max_instruments": PU("platform_proposed", False)}}),
]
RELEASE_START = "2026-09-21T14:00:00.000000000Z"
RELEASE_STEPS = [{"event": "mark", "at": "2026-09-21T14:01:00.000000000Z", "bid": "54000", "session": "crypto", "sane": True},
                 {"event": "goal_complete", "at": "2026-09-21T14:02:00.000000000Z", "session": "crypto"}]

def release_carry():
    """The loss carry MC-R25's release journals, computed by the risk state rather than typed, so MC-V68 and MC-R26
    redeploy on exactly what the connection carries (DEC-270)."""
    rs = RiskState(apply_patch(MB["btc_accumulator"], [rep("/goal/on_complete", "release")]), "0.15", "55000", "crypto",
                   RELEASE_START, mark_max_age_s=doc["harness_defaults"]["mark_max_age_s"])
    journal = [e for s in RELEASE_STEPS for e in rs.step(s)["journal"]]
    return next(e["loss_carry_usd"] for e in journal if e["type"] == "AgentStopped")

RELEASE_CARRY = release_carry()
SEM.append(("MC-V68", "A released agent's loss carry counts against a redeploy on the same connection", "btc_accumulator",
            [rep("/capital/allocation_usd", "1500"), rep("/risk/max_position_usd", "1500"), rep("/risk/max_gross_exposure_usd", "1500")],
            {"connection_loss_carry_usd": RELEASE_CARRY}))

for cid, title, base, patch, ctx in SEM:
    m = apply_patch(MB[base], patch)
    assert V.is_valid(m), (cid, [e.message for e in V.iter_errors(m)])
    errs, warns = semantic(m, dict(CTX, **ctx))
    cases.append({"id": cid, "kind": "semantic", "title": title, "base": base, "patch": patch, "context": ctx,
                  "expect": {"violations": errs, "warnings": warns, "worst_case": worst_case(m)}})

# =========================================================== C. policy
POL = [
    ("MC-P01", "Mandate looser than workspace, tighter than org", "btc_accumulator",
     [rep("/risk/max_drawdown", "0.09"), rep("/risk/drawdown_ladder/2/at", "0.09")],
     [("platform", PLATFORM_BASE), ("organization", {"max_drawdown": "0.1"}), ("workspace", {"max_drawdown": "0.08"})]),
    ("MC-P02", "Mandate equal to the workspace limit", "btc_accumulator", [],
     [("platform", PLATFORM_BASE), ("organization", {"max_drawdown": "0.1"}), ("workspace", {"max_drawdown": "0.08"})]),
    ("MC-P03", "Violating both levels reports the nearest (workspace)", "btc_accumulator",
     [rep("/risk/max_drawdown", "0.1"), rep("/risk/drawdown_ladder/2/at", "0.1")],
     [("platform", PLATFORM_BASE), ("organization", {"max_drawdown": "0.09"}), ("workspace", {"max_drawdown": "0.08"})]),
    ("MC-P04", "Workspace looser than its org is itself invalid", "btc_accumulator", [],
     [("platform", PLATFORM_BASE), ("organization", {"max_drawdown": "0.1"}), ("workspace", {"max_drawdown": "0.12"})]),
    ("MC-P05", "Retail profile: the research agent is not yet allowed to retail", "research_equity", [],
     [("platform", dict(PLATFORM_BASE, **RETAIL_PROFILE)), ("organization", {}), ("workspace", {})]),
    ("MC-P06", "Retail profile: quant-only, ask-only mandate conforms", "btc_accumulator",
     [rep("/autonomy/rules/2/then", "ask")],
     [("platform", dict(PLATFORM_BASE, **RETAIL_PROFILE)), ("organization", {}), ("workspace", {})]),
    ("MC-P07", "Platform maximum on the lifetime loss fraction", "btc_accumulator", [rep("/capital/max_loss_from_allocation", "0.6")],
     [("platform", PLATFORM_BASE), ("organization", {}), ("workspace", {})]),
    ("MC-P08", "Entry threshold below the workspace minimum", "two_stock_swing", [],
     [("platform", PLATFORM_BASE), ("organization", {}), ("workspace", {"entry_threshold": "0.4"})]),
    ("MC-P09", "Org requires protection", "btc_accumulator", [rep("/protection", NO_PROT)],
     [("platform", PLATFORM_BASE), ("organization", {"protection_required": True}), ("workspace", {})]),
    ("MC-P10", "Exits-only rung above the org limit (by action, not index)", "btc_accumulator", [],
     [("platform", PLATFORM_BASE), ("organization", {"exits_only_at_max": "0.05"}), ("workspace", {})]),
    ("MC-P11", "Org requires two approvers above 5000", "btc_accumulator", [],
     [("platform", PLATFORM_BASE), ("organization", {"two_approver_above_usd": "5000"}), ("workspace", {})]),
    ("MC-P12", "Per-model output age above the org maximum", "two_stock_swing", [],
     [("platform", PLATFORM_BASE), ("organization", {"max_output_age_s": 1800}), ("workspace", {})]),
    ("MC-P13", "Rebalance band below the workspace minimum", "btc_accumulator", [],
     [("platform", PLATFORM_BASE), ("organization", {}), ("workspace", {"rebalance_band": "0.1"})]),
    ("MC-P14", "Retail profile: the swing mandate conforms now that auto and llm are allowed", "two_stock_swing", [],
     [("platform", dict(PLATFORM_BASE, **RETAIL_PROFILE)), ("organization", {}), ("workspace", {})]),
    ("MC-P15", "Internal research profile: the thin slice conforms", "research_equity", [],
     [("platform", dict(PLATFORM_BASE, **INTERNAL_RESEARCH_PROFILE)), ("organization", {}), ("workspace", {})]),
    ("MC-P16", "Internal research profile refuses admission auto", "research_equity", [rep("/autonomy/admission", "auto")],
     [("platform", dict(PLATFORM_BASE, **INTERNAL_RESEARCH_PROFILE)), ("organization", {}), ("workspace", {})]),
    ("MC-P17", "No live trading until counsel signs off", "research_equity",
     [rep("/environment", "live"), rep("/connection_id", "conn_alpaca_live_01")],
     [("platform", dict(PLATFORM_BASE, environments=["paper"])), ("organization", {}), ("workspace", {})]),
    ("MC-P18", "Working universe above the org ceiling", "research_equity", [],
     [("platform", PLATFORM_BASE), ("organization", {"max_instruments": 3}), ("workspace", {})]),
    ("MC-P19", "Org caps the research agent weight", "research_equity", [],
     [("platform", PLATFORM_BASE), ("organization", {"research_weight": "0.3"}), ("workspace", {})]),
    ("MC-P20", "Research interval below the org minimum", "research_equity", [],
     [("platform", PLATFORM_BASE), ("organization", {"research_interval_s": 7200}), ("workspace", {})]),
    ("MC-P21", "Revisions per lineage above the workspace maximum", "research_equity", [],
     [("platform", PLATFORM_BASE), ("organization", {}), ("workspace", {"max_revisions_per_lineage": 1})]),
    ("MC-P22", "Research cost cap above the org maximum", "research_equity", [],
     [("platform", PLATFORM_BASE), ("organization", {"research_cost_cap_usd_per_day": "2"}), ("workspace", {})]),
]
for cid, title, base, patch, levels in POL:
    m = apply_patch(MB[base], patch)
    assert V.is_valid(m), cid
    for n, v in levels:
        PV.validate({"policy_schema_version": 1, "level": n, "values": v})
    got = policy_check(m, levels)
    cases.append({"id": cid, "kind": "policy", "title": title, "base": base, "patch": patch,
                  "policies": [{"level": n, "values": v} for n, v in levels], "expect": {"valid": not got, "violations": got}})

# =========================================================== D. risk state
def at(hh, mm, ss=0, day=21):
    return f"2026-09-{day:02d}T{hh:02d}:{mm:02d}:{ss:02d}.000000000Z"

def risk_case(cid, title, base, patch, qty, cost, cls, start, steps, note=None, L="0"):
    m = apply_patch(MB[base], patch)
    assert V.is_valid(m), (cid, [e.message for e in V.iter_errors(m)])
    rs = RiskState(m, qty, cost, cls, start, inherited_loss=L, mark_max_age_s=doc["harness_defaults"]["mark_max_age_s"])
    out = [{**s, "expect": rs.step(s)} for s in steps]
    c = {"id": cid, "kind": "risk_state", "title": title, "base": base, "patch": patch,
         "initial": {"position_qty": qty, "avg_cost": cost, "asset_class": cls, "at": start, "inherited_loss_usd": L}, "steps": out}
    if note:
        c["note"] = note
    cases.append(c)

WIDE = [rep("/risk/max_position_usd", "10000"), rep("/risk/max_position_fraction", "1"), rep("/risk/max_gross_exposure_usd", "10000")]
FAST = [rep("/risk/breach_confirm_s", 0), rep("/risk/scale_lift_after_s", 0), rep("/risk/daily_breach_min_s", 0)]
ISO = [rep("/risk/max_daily_loss", "0.5")]
def mk(t, b, sess="regular", sane=True):
    return {"event": "mark", "at": t, "bid": b, "session": sess, "sane": sane}
def clk(t, sess="regular"):
    return {"event": "clock", "at": t, "session": sess}

risk_case("MC-R01", "Drawdown ladder: scale, hysteresis boundary, exits_only, flatten_and_pause", "two_stock_swing",
          WIDE + FAST + ISO, "100", "100", "us_equity", at(14, 0),
          [mk(at(14, 1), "105"), mk(at(14, 2), "101.85"), mk(at(14, 3), "101"), mk(at(14, 4), "102"),
           mk(at(14, 5), "102.9"), mk(at(14, 6), "103"), mk(at(14, 7), "98.7"), mk(at(14, 8), "96.6")],
          note="Confirmation and lift delays are 0 and max_daily_loss is 0.5 to isolate the ladder.")
risk_case("MC-R02", "Time-in-breach confirmation: short recoveries do not restart it", "two_stock_swing",
          WIDE + ISO + [rep("/risk/scale_lift_after_s", 0)], "100", "100", "us_equity", at(14, 0),
          [mk(at(14, 1), "105"), mk(at(14, 2), "98.5"), mk(at(14, 2, 30), "98.6"), mk(at(14, 2, 45), "99"),
           mk(at(14, 2, 50), "98.6"), mk(at(14, 3, 10), "98.4"), mk(at(14, 3, 20), "98.2")],
          note="exits_only rung at E <= 9870. In breach 14:02:00-14:02:45 (45 s) and from 14:02:50; the 5 s recovery is "
               "shorter than breach_confirm_s (60 s) so time accumulates: 45 + 20 = 65 s at 14:03:10.")
risk_case("MC-R03", "Recovery longer than breach_confirm_s restarts confirmation", "two_stock_swing",
          WIDE + ISO + [rep("/risk/scale_lift_after_s", 0)], "100", "100", "us_equity", at(14, 0),
          [mk(at(14, 1), "105"), mk(at(14, 2), "98.5"), mk(at(14, 2, 30), "99"), mk(at(14, 3, 40), "98.6"),
           mk(at(14, 4, 20), "98.6"), mk(at(14, 4, 40), "98.6")])
risk_case("MC-R04", "Hard trigger at 1.25 x the rung: temporary exits_only, latched on a second quote", "two_stock_swing",
          WIDE + ISO + [rep("/risk/scale_lift_after_s", 0)], "100", "100", "us_equity", at(14, 0),
          [mk(at(14, 1), "105"), mk(at(14, 2), "97.5"), mk(at(14, 2, 5), "97.1"), mk(at(14, 2, 15), "97")],
          note="exits_only hard level: H - E >= 1.25 x 0.06 x 10500 = 787.5, so E <= 9712.5. The first hard quote (14:02:05) "
               "applies a temporary exits_only; the second, 10 s later, latches the rung.")
risk_case("MC-R05", "Confirmation continues on clock ticks after the close", "two_stock_swing",
          WIDE + [rep("/risk/scale_lift_after_s", 0), rep("/risk/daily_breach_min_s", 0)], "20", "150", "us_equity", at(4, 0),
          [{"event": "risk_day_started", "at": at(4, 0), "session": "after_hours"}, mk(at(19, 59, 30), "139.9"),
           clk(at(20, 0, 10), "after_hours"), clk(at(20, 0, 40), "after_hours")],
          note="Daily breach at 15:59:30 ET, the last regular-session mark. Breach time reaches 60 s after the close; the limit "
               "triggers at the first input at or after that (the 16:00:40 clock tick).")
risk_case("MC-R06", "A pending daily breach is resolved at the risk-day rollover", "btc_accumulator",
          [], "0.1", "60000", "crypto", at(4, 0),
          [{"event": "risk_day_started", "at": at(4, 0), "session": "crypto"}, mk(at(3, 59, 30, day=22), "57990", "crypto"),
           {"event": "risk_day_started", "at": at(4, 0, day=22), "session": "crypto"}, mk(at(4, 30, day=22), "58000", "crypto"),
           mk(at(5, 0, day=22), "58000", "crypto")],
          note="The breach starts 30 s before midnight (breach_confirm_s 60). After the rollover it keeps confirming against the "
               "previous day's E0 and latches (resolved_at_rollover) once it has 60 s of breach time; it lifts at the next midnight.")
risk_case("MC-R07", "Daily loss: lift waits daily_breach_min_s after a late-night crypto breach", "btc_accumulator",
          [rep("/risk/breach_confirm_s", 0)], "0.1", "60000", "crypto", at(4, 0),
          [{"event": "risk_day_started", "at": at(4, 0), "session": "crypto"}, mk(at(3, 50, day=22), "57900", "crypto"),
           {"event": "risk_day_started", "at": at(4, 0, day=22), "session": "crypto"}, mk(at(4, 30, day=22), "58000", "crypto"),
           mk(at(4, 50, day=22), "58000", "crypto")],
          note="23:50 America/New_York is 03:50Z; daily_breach_min_s is 3600.")
risk_case("MC-R08", "New-day breach during the lift delay renews the latch", "btc_accumulator",
          [rep("/risk/breach_confirm_s", 0)], "0.1", "60000", "crypto", at(4, 0),
          [{"event": "risk_day_started", "at": at(4, 0), "session": "crypto"}, mk(at(3, 50, day=22), "57900", "crypto"),
           {"event": "risk_day_started", "at": at(4, 0, day=22), "session": "crypto"}, mk(at(4, 20, day=22), "55800", "crypto"),
           mk(at(5, 30, day=22), "55800", "crypto")])
risk_case("MC-R09", "Acknowledgment: rejected while flattening; reset after flat; rungs lift one at a time", "two_stock_swing",
          WIDE + ISO + [rep("/risk/breach_confirm_s", 0),
                        rep("/risk/drawdown_ladder", [{"at": "0.02", "action": "scale_sizes", "factor": "0.75"},
                                                      {"at": "0.04", "action": "scale_sizes", "factor": "0.5"},
                                                      {"at": "0.06", "action": "exits_only", "factor": None},
                                                      {"at": "0.08", "action": "flatten_and_pause", "factor": None}])],
          "100", "100", "us_equity", at(14, 0),
          [mk(at(14, 1), "105"), mk(at(14, 2), "96.6"),
           {"event": "owner_acknowledged", "at": at(14, 3), "restriction": "drawdown_ladder", "session": "regular"},
           {"event": "fill", "at": at(14, 4), "side": "sell", "qty": "100", "price": "96.5", "session": "regular"},
           {"event": "owner_acknowledged", "at": at(14, 10), "restriction": "drawdown_ladder", "session": "regular"},
           clk(at(14, 15)), clk(at(14, 20)), clk(at(14, 25)), clk(at(14, 30))],
          note="After the reset both scale rungs are active; the higher rung (0.04, factor 0.5) lifts first after 600 s, then the lower.")
risk_case("MC-R10", "Lifetime floor with an inherited loss; cannot be acknowledged", "two_stock_swing",
          WIDE + ISO + [rep("/risk/breach_confirm_s", 0)], "100", "100", "us_equity", at(14, 0),
          [mk(at(14, 1), "97"), mk(at(14, 2), "93"),
           {"event": "owner_acknowledged", "at": at(14, 3), "restriction": "lifetime_floor", "session": "regular"}],
          L="300", note="Floor: E <= C x (1 - 0.1) + L = 9000 + 300 = 9300.")
risk_case("MC-R11", "Allocation changes scale H, E0, and the capital base; a withdrawal never trips the floor", "two_stock_swing",
          WIDE + FAST + ISO, "10", "100", "us_equity", at(14, 0),
          [mk(at(14, 1), "50"),
           {"event": "allocation_change", "at": at(14, 2), "delta_usd": "-5000", "session": "regular"},
           {"event": "allocation_change", "at": at(14, 3), "delta_usd": "5000", "session": "regular"}],
          note="Reviewer counterexample: E = 9500, C = 10000; withdrawing 5000 used to set the floor to 4500 = E.")
risk_case("MC-R12", "Allocation rejected: increase while latched; decrease below exposure", "two_stock_swing",
          WIDE + FAST + ISO, "100", "100", "us_equity", at(14, 0),
          [mk(at(14, 1), "105"), mk(at(14, 2), "98.7"),
           {"event": "allocation_change", "at": at(14, 3), "delta_usd": "1000", "session": "regular"},
           {"event": "allocation_change", "at": at(14, 4), "delta_usd": "-1000", "session": "regular"}])
risk_case("MC-R13", "Session marks ignored; failed and missing marks set a per-instrument stale restriction", "two_stock_swing",
          WIDE + FAST + ISO, "100", "100", "us_equity", at(13, 31),
          [mk(at(13, 32), "100"), mk(at(19, 59, 30), "100"), mk(at(12, 0, day=22), "80", "pre_market"), mk(at(13, 31, day=22), "99", sane=False),
           mk(at(13, 32, day=22), "99"), clk(at(13, 34, day=22))],
          note="mark_max_age_s is 120 s of regular-session time.")
risk_case("MC-R14", "Daily flatten_and_pause: acknowledgment after flat leaves exits_only until the next day", "two_stock_swing",
          WIDE + [rep("/risk/breach_confirm_s", 0), rep("/risk/daily_loss_action", "flatten_and_pause"), rep("/risk/daily_breach_min_s", 0)],
          "20", "150", "us_equity", at(4, 0),
          [{"event": "risk_day_started", "at": at(4, 0), "session": "after_hours"}, mk(at(14, 0), "140"),
           {"event": "fill", "at": at(14, 1), "side": "sell", "qty": "20", "price": "140", "session": "regular"},
           {"event": "owner_acknowledged", "at": at(14, 30), "restriction": "daily_loss", "session": "regular"},
           {"event": "risk_day_started", "at": at(4, 0, day=22), "session": "after_hours"}])
risk_case("MC-R15", "Ladder and daily loss in one step; the stricter mode holds after the daily lift", "two_stock_swing",
          WIDE + FAST, "100", "100", "us_equity", at(4, 0),
          [{"event": "risk_day_started", "at": at(4, 0), "session": "after_hours"}, mk(at(14, 0), "98"), mk(at(14, 5), "94"),
           {"event": "risk_day_started", "at": at(4, 0, day=22), "session": "after_hours"}])

derived("btc_accumulator_disarm", "btc_accumulator", [rep("/goal/on_complete", "disarm_ladder")], "on_complete disarm_ladder")
derived("btc_accumulator_release", "btc_accumulator", [rep("/goal/on_complete", "release")], "on_complete release")
risk_case("MC-R16", "on_complete disarm_ladder: holding with the ladder and daily loss disarmed; floor stays armed", "btc_accumulator_disarm",
          [rep("/risk/breach_confirm_s", 0)], "0.15", "55000", "crypto", at(14, 0),
          [{"event": "goal_complete", "at": at(14, 1), "session": "crypto"}, mk(at(14, 2), "50600", "crypto"),
           mk(at(14, 3), "48000", "crypto")],
          note="E = 10000 + 0.15 x (mark - 55000). At 50600 DD = 0.066 (would be exits_only) and daily -6.6%; disarmed. "
               "At 48000 E = 8950 <= floor 9000.")
risk_case("MC-R17", "on_complete release: positions released and the agent retires", "btc_accumulator_release",
          [], "0.15", "55000", "crypto", at(14, 0), [{"event": "goal_complete", "at": at(14, 1), "session": "crypto"}])

risk_case("MC-R18", "Flatten hard trigger needs a second quote: exits_only first, flatten 10 s later", "two_stock_swing",
          WIDE + ISO + [rep("/risk/scale_lift_after_s", 0)], "100", "100", "us_equity", at(14, 0),
          [mk(at(14, 1), "105"), mk(at(14, 2), "94.4"), mk(at(14, 2, 5), "94.3"), mk(at(14, 2, 12), "94.2")],
          note="Flatten hard level: H - E >= 1.25 x 0.08 x 10500 = 1050, so E <= 9450. The exits_only rung hard-triggers at once; "
               "the flatten needs the hard level on a quote at least min(breach_confirm_s, 10) = 10 s after the first.")
risk_case("MC-R19", "A single flash print does not flatten", "two_stock_swing",
          WIDE + ISO + [rep("/risk/scale_lift_after_s", 0)], "100", "100", "us_equity", at(14, 0),
          [mk(at(14, 1), "105"), mk(at(14, 2), "94.4"), mk(at(14, 2, 1), "104")])
risk_case("MC-R20", "A flash breach just before midnight is discarded after the rollover", "btc_accumulator",
          [], "0.1", "60000", "crypto", at(4, 0),
          [{"event": "risk_day_started", "at": at(4, 0), "session": "crypto"}, mk(at(3, 59, 59, day=22), "57990", "crypto"),
           {"event": "risk_day_started", "at": at(4, 0, day=22), "session": "crypto"}, mk(at(4, 0, 1, day=22), "60000", "crypto"),
           clk(at(4, 1, 30, day=22), "crypto")])
risk_case("MC-R21", "Loosening a latched floor: waiting period, insufficient, then lifted", "two_stock_swing",
          WIDE + ISO + [rep("/risk/breach_confirm_s", 0)], "100", "100", "us_equity", at(14, 0),
          [mk(at(14, 1), "90"),
           {"event": "fill", "at": at(14, 2), "side": "sell", "qty": "100", "price": "90", "session": "regular"},
           {"event": "floor_loosened", "at": at(14, 30), "new_max_loss_from_allocation": "0.2", "independent_approval": False,
            "confirmed_at": at(14, 20), "session": "regular"},
           {"event": "floor_loosened", "at": at(14, 31), "new_max_loss_from_allocation": "0.1", "independent_approval": True,
            "confirmed_at": at(14, 20), "session": "regular"},
           {"event": "floor_loosened", "at": at(14, 32), "new_max_loss_from_allocation": "0.2", "independent_approval": True,
            "confirmed_at": at(14, 20), "session": "regular"}],
          note="Floor 9000 at f = 0.1; E = 9000 latches it. Raising f to 0.2 moves the floor to 8000 (E > 8000, lifts) but a "
               "single approver must wait one full risk day. The second attempt is not a loosening.")
risk_case("MC-R22", "profit_stop confirms by time in breach inside the risk state", "two_stock_swing",
          WIDE + ISO, "100", "100", "us_equity", at(14, 0),
          [mk(at(14, 1), "110"), mk(at(14, 1, 30), "109.9"), mk(at(14, 1, 40), "110.1"), mk(at(14, 2, 10), "110.2")],
          note="profit_level 0.1: E - C >= 1000. In breach 14:01:00-14:01:30 (30 s) and from 14:01:40; the 10 s dip is shorter "
               "than breach_confirm_s, so 30 + 30 = 60 s at 14:02:10.")
risk_case("MC-R23", "Withdrawals cannot shrink the loss carried to the connection", "btc_accumulator",
          [rep("/risk/breach_confirm_s", 0), rep("/risk/max_daily_loss", "0.5")], "0.15", "60000", "crypto", at(14, 0),
          [mk(at(14, 1), "54333.34", "crypto"),
           {"event": "fill", "at": at(14, 2), "side": "sell", "qty": "0.15", "price": "54333.34", "session": "crypto"},
           {"event": "allocation_change", "at": at(14, 3), "delta_usd": "-9000", "session": "crypto"},
           {"event": "agent_stopped", "at": at(14, 4), "session": "crypto"}],
          note="Reviewer probe: the agent loses about 850, is flat, withdraws 9000, and retires. The carry is the net dollar loss.")

risk_case("MC-R25", "on_complete release with a loss: the connection carries max(0, N - E)", "btc_accumulator_release",
          [], "0.15", "55000", "crypto", RELEASE_START, RELEASE_STEPS,
          note="E = 10000 + 0.15 x (54000 - 55000) = 9850 and N = 10000, so the release retires the agent with a carry of 150, "
               "as agent_stopped would (DEC-270, MI-14).")
risk_case("MC-R26", "A redeploy after a release opens at the carried L", "btc_accumulator",
          ISO + [rep("/risk/breach_confirm_s", 0), rep("/capital/max_loss_from_allocation", "0.05")], "0.1", "60000", "crypto",
          at(14, 0), [mk(at(14, 1), "56600", "crypto"), mk(at(14, 2), "56500", "crypto")], L=RELEASE_CARRY,
          note="L is MC-R25's carry, 150. Floor: E <= C x (1 - 0.05) + L = 9500 + 150 = 9650. E = 9660 at 56600 stays above it; "
               "E = 9650 at 56500 latches it, 150 above the floor a fresh connection would have (§5.7, V-032).")
risk_case("MC-R24", "A removed instrument is exits-only in that instrument; re-admission clears it", "research_equity",
          [], "10", "100", "us_equity", at(14, 0),
          [mk(at(14, 1), "100"),
           {"event": "universe_changed", "at": at(14, 2), "instrument": XYZ, "change": "removed",
            "reason": "thesis_expired", "session": "regular"},
           mk(at(14, 3), "100"),
           {"event": "universe_changed", "at": at(14, 4), "instrument": XYZ, "change": "admitted",
            "reason": "thesis_admitted", "session": "regular"}],
          note="A universe change is an account-stream risk input (DEC-97). Removal restricts only that instrument: "
               "protection stays and exits are never denied (MI-1, MI-19).")

# =========================================================== E. risk day boundaries
for cid, title, instant in [
    ("MC-T01", "Last second of 2026-03-07 (EST)", "2026-03-08T04:59:59.000000000Z"),
    ("MC-T02", "DST start day is 23 hours", "2026-03-08T05:00:00.000000000Z"),
    ("MC-T03", "First full EDT day", "2026-03-09T04:00:00.000000000Z"),
    ("MC-T04", "DST end day is 25 hours", "2026-11-01T04:00:00.000000000Z"),
    ("MC-T05", "First full EST day", "2026-11-02T05:00:00.000000000Z"),
]:
    cases.append({"id": cid, "kind": "risk_day", "title": title, "at": instant, "expect": risk_day(instant)})

# =========================================================== F. gate
GS = {"now": at(15, 0), "agent_equity": "10000", "positions_mv": {XYZ: "1000"},
      "working_opening_orders": [{"instrument": XYZ, "max_cost": "300"}], "orders_today": 3,
      "last_exit_fill_at": {}, "working_universe": [QRS, XYZ]}
G = [
    ("MC-G01", "Per-instrument cap exceeded (current + working + proposed)", {}, GS, (XYZ, "increase", "3")),
    ("MC-G02", "Per-instrument cap met exactly", {}, GS, (XYZ, "increase", "2")),
    ("MC-G03", "Order size above max_order_usd", {}, GS, (QRS, "open", "11")),
    ("MC-G04", "Agent gross exposure exceeded (working order in another instrument counts)", {}, GS, (QRS, "open", "8")),
    ("MC-G05", "Fraction cap binds when equity falls", {"/risk/max_position_fraction": "0.15"},
     dict(GS, agent_equity="9500", positions_mv={XYZ: "1300"}, working_opening_orders=[]), (XYZ, "increase", "2")),
    ("MC-G06", "Equity below max_gross_exposure_usd caps gross exposure",
     {"/risk/max_position_usd": "10000", "/risk/max_position_fraction": "1", "/risk/max_gross_exposure_usd": "10000"},
     dict(GS, agent_equity="9500", positions_mv={XYZ: "9000"}, working_opening_orders=[]), (QRS, "open", "6")),
    ("MC-G07", "Orders per day reached", {"/risk/max_orders_per_day": 3}, GS, (QRS, "open", "1")),
    ("MC-G08", "Exit is allowed when orders per day is at the limit", {"/risk/max_orders_per_day": 3}, GS, (XYZ, "discretionary_exit", "10")),
    ("MC-G09", "Risk exit larger than max_order_usd is allowed", {}, dict(GS, positions_mv={XYZ: "1200"}), (XYZ, "risk_exit", "12")),
    ("MC-G10", "Owner exit is allowed by every mandate limit", {"/risk/max_orders_per_day": 3}, dict(GS, positions_mv={XYZ: "1200"}),
     (XYZ, "owner_exit", "12")),
    ("MC-G11", "Re-entry cooldown after an exit fill", {}, dict(GS, last_exit_fill_at={QRS: at(14, 30)}), (QRS, "open", "1")),
    ("MC-G12", "Re-entry cooldown applies across the instrument group", {},
     dict(GS, last_exit_fill_at={LMN: at(14, 30)}, instrument_groups={QRS: "grp_q", LMN: "grp_q"}), (QRS, "open", "1")),
    ("MC-G13", "Re-entry allowed once the cooldown has passed", {}, dict(GS, now=at(15, 30), last_exit_fill_at={QRS: at(14, 30)}),
     (QRS, "open", "1")),
    ("MC-G14", "Opening an instrument outside the working universe", {}, dict(GS, working_universe=[XYZ]), (QRS, "open", "1")),
    ("MC-G15", "Exiting an instrument outside the working universe is allowed", {}, dict(GS, working_universe=[XYZ]),
     (QRS, "discretionary_exit", "1")),
    ("MC-G16", "An empty working universe denies every opening", {}, dict(GS, working_universe=[]), (XYZ, "increase", "1")),
]
for cid, title, over, st, (inst, purpose, qty) in G:
    patch = [rep(p, v) for p, v in over.items()]
    m = apply_patch(swing, patch)
    assert V.is_valid(m), cid
    prop = {"instrument": inst, "purpose": purpose, "qty": qty, "limit_price": "100"}
    cases.append({"id": cid, "kind": "gate", "title": title, "base": "two_stock_swing", "patch": patch, "state": st,
                  "proposed": prop, "expect": gate(m, st, prop)})

# =========================================================== G. order builder
NOW = "2026-09-22T14:00:00.000000000Z"
def out(model, conv, conf, as_of="2026-09-22T13:59:00.000000000Z", exp="2026-09-22T15:00:00.000000000Z", inst=XYZ):
    reg = REGISTRY[model]
    return {"model_id": model, "model_version": reg["version"], "content_hash": reg["content_hash"], "instrument_id": inst,
            "as_of": as_of, "expires_at": exp, "conviction": conv, "confidence": conf}
MOM, NEWS, MR = "quant.momentum", "llm.news_research", "quant.mean_reversion"
GST = {"agent_equity": "10000", "positions_mv": {}, "working_opening_orders": [], "orders_today": 0,
       "last_exit_fill_at": {}, "working_universe": [BTC, QRS, XYZ]}
def gst(**kw):
    return dict(GST, **kw)
def at_now(inp, now):
    """`inp` with its clock at `now`: each output keeps its `as_of` and `expires_at` offsets from `now`."""
    shift = T(now) - T(inp["now"])
    return dict(inp, now=now, outputs=[dict(o, as_of=fmt(T(o["as_of"]) + shift), expires_at=fmt(T(o["expires_at"]) + shift))
                                       for o in inp["outputs"]])
AFTER_HOURS = "2026-09-22T21:00:00.000000000Z"
CLOSE_WINDOW = "2026-09-22T19:55:00.000000000Z"
BI = {"now": NOW, "instrument": XYZ, "asset_class": "us_equity", "agent_equity": "10000", "position_qty": "0",
      "quote": {"bid": "99.9", "ask": "100"}, "qty_increment": "1", "min_order_usd": "1", "gate_state": GST}
TWO = [out(MOM, "0.8", "0.9"), out(NEWS, "0.2", "0.5")]
CR = dict(BI, instrument=BTC, asset_class="crypto", session="crypto", qty_increment="0.0001")
derived("two_stock_swing_no_avg_down", "two_stock_swing",
        [{"op": "add", "path": "/autonomy/rules/0", "value": {"id": "no_averaging_down", "when": {"all": [
            {"field": "purpose", "op": "eq", "value": "increase"},
            {"field": "position_pnl_fraction", "op": "lt", "value": "0"}]}, "then": "deny"}}], "adds rule no_averaging_down first")
derived("two_stock_swing_trim", "two_stock_swing", [rep("/risk/scale_action", "trim_to_target")], "scale_action trim_to_target")
B = [
    ("MC-B01", "Two models, open, AUTO by the routine rule", "two_stock_swing", dict(BI, outputs=TWO)),
    ("MC-B02", "Same outputs with the 0.5 ladder factor", "two_stock_swing", dict(BI, size_factor="0.5", outputs=TWO)),
    ("MC-B03", "Between thresholds: hold", "two_stock_swing", dict(BI, outputs=[out(MOM, "0.3", "0.9"), out(NEWS, "0.2", "0.5")])),
    ("MC-B04", "Below the exit threshold: discretionary exit, AUTO built-in", "two_stock_swing",
     dict(BI, position_qty="5", gate_state=gst(positions_mv={XYZ: "499.5"}), outputs=[out(MOM, "-0.8", "0.9"), out(NEWS, "-0.2", "0.5")])),
    ("MC-B05", "Increase an existing position", "two_stock_swing",
     dict(BI, position_qty="3", gate_state=gst(positions_mv={XYZ: "299.7"}), outputs=TWO)),
    ("MC-B06", "Missing model counts as fully bearish for buys: hold", "two_stock_swing",
     dict(BI, outputs=[out(MOM, "0.8", "0.9"), out(NEWS, "0.2", "0.5", exp=NOW)])),
    ("MC-B07", "Missing model counts as zero for exits: still exits", "two_stock_swing",
     dict(BI, position_qty="5", gate_state=gst(positions_mv={XYZ: "499.5"}), outputs=[out(MOM, "-0.9", "0.9")])),
    ("MC-B08", "Low combined score: ASK", "two_stock_swing", dict(BI, outputs=[out(MOM, "0.9", "0.6"), out(NEWS, "0.9", "0.6")])),
    ("MC-B09", "Future as_of is ignored", "two_stock_swing",
     dict(BI, outputs=[out(MOM, "0.9", "0.9"), out(NEWS, "0.9", "0.9"), out(NEWS, "-1", "1", as_of="2026-09-22T14:00:01.000000000Z")])),
    ("MC-B10", "Output older than the model's max_output_age_s is ignored", "two_stock_swing",
     dict(BI, outputs=[out(MOM, "0.9", "0.9", as_of="2026-09-22T13:44:59.000000000Z"), out(NEWS, "0.9", "0.9")])),
    ("MC-B11", "Duplicate outputs: the latest per model wins", "two_stock_swing",
     dict(BI, outputs=TWO + [out(MOM, "0.1", "0.9", as_of="2026-09-22T13:58:00.000000000Z")])),
    ("MC-B12", "Wrong model version is ignored (counts as missing)", "two_stock_swing",
     dict(BI, outputs=[out(MOM, "0.9", "0.9"), dict(out(NEWS, "0.9", "0.9"), model_version="0.2.0")])),
    ("MC-B13", "Score rounds to 12 places before the rule compares it", "two_stock_swing",
     dict(BI, outputs=[out(MOM, "1", "0.65"), out(NEWS, "1", "0.6499999999999")])),
    ("MC-B14", "Clipped to max_order_usd", "two_stock_swing", dict(BI, outputs=[out(MOM, "1", "1"), out(NEWS, "1", "1")])),
    ("MC-B15", "Working opening order counts toward the target", "two_stock_swing",
     dict(BI, working_opening_orders=[{"instrument": XYZ, "max_cost": "700"}],
          gate_state=gst(working_opening_orders=[{"instrument": XYZ, "max_cost": "700"}]), outputs=TWO)),
    ("MC-B16", "Above target with positive conviction and limit_buys: hold", "two_stock_swing",
     dict(BI, position_qty="10", size_factor="0.5", gate_state=gst(positions_mv={XYZ: "999"}), outputs=TWO)),
    ("MC-B17", "trim_to_target: confirmed rung reduces the position as a risk exit", "two_stock_swing_trim",
     dict(BI, position_qty="10", size_factor="0.5", scale_active_s=120, gate_state=gst(positions_mv={XYZ: "999"}), outputs=TWO)),
    ("MC-B30", "trim_to_target withheld until the rung is confirmed", "two_stock_swing_trim",
     dict(BI, position_qty="10", size_factor="0.5", scale_active_s=10, gate_state=gst(positions_mv={XYZ: "999"}), outputs=TWO)),
    ("MC-B31", "trim_to_target withheld outside the regular session and while holding", "two_stock_swing_trim",
     at_now(dict(BI, position_qty="10", size_factor="0.5", scale_active_s=120, session="after_hours", holding=True,
                 gate_state=gst(positions_mv={XYZ: "999"}), outputs=TWO), AFTER_HOURS)),
    ("MC-B32", "No trim when the excess is below the rebalance band", "two_stock_swing_trim",
     dict(BI, position_qty="8", size_factor="0.5", scale_active_s=120, gate_state=gst(positions_mv={XYZ: "799.2"}), outputs=TWO)),
    ("MC-B18", "Delta within the rebalance band: hold", "two_stock_swing", dict(BI, position_qty="7", gate_state=gst(positions_mv={XYZ: "699.3"}), outputs=TWO)),
    ("MC-B19", "Value after limit clips below the band: hold", "two_stock_swing",
     dict(BI, gross_usd="1950", gate_state=gst(positions_mv={QRS: "1950"}), outputs=[out(MOM, "1", "1"), out(NEWS, "1", "1")])),
    ("MC-B20", "No fresh outputs: hold", "two_stock_swing", dict(BI, outputs=[])),
    ("MC-B21", "Gate dry run denies: no ASK is sent", "two_stock_swing",
     dict(BI, outputs=[out(MOM, "0.9", "0.6"), out(NEWS, "0.9", "0.6")], gate_state=gst(orders_today=50))),
    ("MC-B22", "Discretionary exit outside the regular session is deferred", "two_stock_swing",
     at_now(dict(BI, session="after_hours", position_qty="5", gate_state=gst(positions_mv={XYZ: "499.5"}),
                 outputs=[out(MOM, "-0.8", "0.9"), out(NEWS, "-0.2", "0.5")]), AFTER_HOURS)),
    ("MC-B23", "Discretionary exit in the close window goes out as a marketable limit", "two_stock_swing",
     at_now(dict(BI, in_close_window=True, position_qty="5", gate_state=gst(positions_mv={XYZ: "499.5"}),
                 outputs=[out(MOM, "-0.8", "0.9"), out(NEWS, "-0.2", "0.5")]), CLOSE_WINDOW)),
    ("MC-B24", "Averaging-down rule denies an increase", "two_stock_swing_no_avg_down",
     dict(BI, position_qty="3", position_pnl_fraction="-0.04", gate_state=gst(positions_mv={XYZ: "299.7"}), outputs=TWO)),
    ("MC-B25", "Re-entry after a round trip is not a first trade", "two_stock_swing_no_avg_down",
     dict(BI, has_prior_fill=True, outputs=TWO)),
    ("MC-B26", "Accumulate clipped to the remaining target quantity", "btc_accumulator",
     dict(CR, position_qty="0.14", cost_basis_usd="7700", goal_spent_usd="7700", quote={"bid": "54990", "ask": "55000"},
          gate_state=gst(positions_mv={BTC: "7698.6"}), outputs=[out(MR, "1", "1", inst=BTC)])),
    ("MC-B27", "Accumulate clipped by max_avg_price", "btc_accumulator",
     dict(CR, position_qty="0.05", cost_basis_usd="2890", goal_spent_usd="2890", quote={"bid": "59990", "ask": "60000"},
          gate_state=gst(positions_mv={BTC: "2999.5"}), outputs=[out(MR, "0.9", "0.9", inst=BTC)])),
    ("MC-B28", "Accumulate with fees: quantity received and spend include fees", "btc_accumulator",
     dict(CR, position_qty="0.14", cost_basis_usd="7700", goal_spent_usd="7700", quote={"bid": "54990", "ask": "55000"},
          fee_rate_asset="0.0025", qty_increment="0.000001", gate_state=gst(positions_mv={BTC: "7698.6"}),
          outputs=[out(MR, "1", "1", inst=BTC)])),
    ("MC-B29", "Accumulate never sells on negative conviction", "btc_accumulator",
     dict(CR, position_qty="0.05", quote={"bid": "54990", "ask": "55000"}, gate_state=gst(positions_mv={BTC: "2749.5"}),
          outputs=[out(MR, "-0.9", "0.9", inst=BTC)])),
]
for cid, title, base, inp in B:
    cases.append({"id": cid, "kind": "builder", "title": title, "base": base, "input": inp, "expect": builder(MB[base], inp)})

# =========================================================== H. autonomy
A = [
    ("MC-A01", "Discretionary exit is AUTO regardless of rules", {}, {"purpose": "discretionary_exit"}),
    ("MC-A02", "Protective order is AUTO", {}, {"purpose": "protective"}),
    ("MC-A03", "Risk exit is AUTO", {}, {"purpose": "risk_exit"}),
    ("MC-A04", "Owner exit is AUTO", {}, {"purpose": "owner_exit"}),
    ("MC-A05", "Large open: ASK by large_orders", {}, {"order_usd": "950", "combined_score": "0.9"}),
    ("MC-A06", "Low score open: ASK", {}, {"order_usd": "300", "combined_score": "0.6"}),
    ("MC-A07", "Routine open: AUTO", {}, {"order_usd": "300", "combined_score": "0.8"}),
    ("MC-A08", "Order exactly at the threshold is not large", {}, {"order_usd": "900", "combined_score": "0.8"}),
    ("MC-A09", "No rule matches: default ask", {"/autonomy/rules": []}, {"order_usd": "100", "combined_score": "0.9"}),
    ("MC-A10", "Two approvers above the threshold", {"/autonomy/approval/two_approver_above_usd": "500"},
     {"order_usd": "600", "combined_score": "0.6"}),
    ("MC-A11", "Bought-today rule catches order splitting",
     {"/autonomy/rules": [{"id": "daily_buys", "when": {"field": "bought_today_usd", "op": "gt", "value": "2000"}, "then": "ask"}] + RULES},
     {"order_usd": "500", "combined_score": "0.9", "bought_today_usd": "2300"}),
    ("MC-A12", "The admission ceiling turns an auto rule into ask for a new instrument", {},
     {"order_usd": "300", "combined_score": "0.8", "new_instrument": True, "thesis_confidence": "0.9"}),
    ("MC-A13", "Admission auto confirmed by the owner is AUTO", {"/autonomy/admission": "auto"},
     {"order_usd": "300", "combined_score": "0.8", "new_instrument": True, "thesis_confidence": "0.9"}),
    ("MC-A14", "Admission deny overrides an auto rule", {"/autonomy/admission": "deny"},
     {"order_usd": "300", "combined_score": "0.8", "new_instrument": True, "thesis_confidence": "0.9"}),
    ("MC-A15", "The admission ceiling never loosens a deny rule", {"/autonomy/admission": "auto",
      "/autonomy/rules": [{"id": "no_new", "when": {"field": "new_instrument", "op": "eq", "value": True}, "then": "deny"}] + RULES},
     {"order_usd": "300", "combined_score": "0.8", "new_instrument": True, "thesis_confidence": "0.9"}),
    ("MC-A16", "A thesis-confidence rule asks below the owner threshold", {"/autonomy/admission": "auto",
      "/autonomy/rules": [{"id": "thin_thesis", "when": {"field": "thesis_confidence", "op": "lt", "value": "0.6"}, "then": "ask"}] + RULES},
     {"order_usd": "300", "combined_score": "0.8", "new_instrument": True, "thesis_confidence": "0.5"}),
]
for cid, title, over, a in A:
    patch = [rep(p, v) for p, v in over.items()]
    m = apply_patch(btc, patch)
    assert V.is_valid(m), cid
    full = {"purpose": "open", "order_usd": "0", "combined_score": "0", "instrument": BTC, "asset_class": "crypto",
            "session": "crypto", "first_trade_in_instrument": False, "drawdown": "0", "daily_pnl_fraction": "0",
            "position_usd_after": "0", "gross_usd_after": "0", "bought_today_usd": "0", "position_pnl_fraction": "0",
            "new_instrument": False, "thesis_confidence": "0"}
    full.update(a)
    if full["purpose"] in REDUCING:
        full = {"purpose": full["purpose"]}
    cases.append({"id": cid, "kind": "autonomy", "title": title, "base": "btc_accumulator", "patch": patch, "action": full,
                  "expect": autonomy(m, full)})

# =========================================================== I. agent-scoped kill switch
FLI = {"agent": "agent_a",
       "open_orders": [{"client_order_id": "a-1", "agent": "agent_a", "instrument": XYZ},
                       {"client_order_id": "a-2", "agent": "agent_a", "instrument": BTC},
                       {"client_order_id": "b-1", "agent": "agent_b", "instrument": QRS}],
       "agent_positions": [{"agent": "agent_a", "instrument": XYZ, "asset_class": "us_equity", "qty": "10"},
                           {"agent": "agent_a", "instrument": BTC, "asset_class": "crypto", "qty": "0.05"},
                           {"agent": "agent_b", "instrument": QRS, "asset_class": "us_equity", "qty": "20"}],
       "broker_positions": [{"instrument": XYZ, "qty": "15"}, {"instrument": BTC, "qty": "0.05"}, {"instrument": QRS, "qty": "20"}]}
for cid, title, extra in [
    ("MC-F01", "Automated flatten on a shared account touches only that agent", {"session": "regular", "initiator": "risk_limit"}),
    ("MC-F02", "Automated flatten after hours: equity sells wait; crypto sells go now", {"session": "after_hours", "initiator": "risk_limit"}),
    ("MC-F03", "Owner kill switch after hours with the bid confirmed: sells now, never below the floor price",
     {"session": "after_hours", "initiator": "owner", "owner_confirmed_bid": True, "confirmed_bid": "100", "max_exit_offset": "0.03"}),
    ("MC-F04", "Owner kill switch after hours without confirmation: equity sells wait", {"session": "after_hours", "initiator": "owner", "owner_confirmed_bid": False}),
]:
    inp = dict(FLI, **extra)
    cases.append({"id": cid, "kind": "agent_flatten", "title": title, "input": inp, "expect": agent_flatten(inp),
                  "note": "Broker XYZ is 15 but agent_a's sub-ledger is 10; 5 shares are the owner's."})

# =========================================================== J. goals
GL = [
    ("MC-L01", "accumulate reaches target_qty", "btc_accumulator",
     {"now": at(15, 0), "position_qty": "0.15", "goal_spent_usd": "8300", "min_order_usd": "1", "qty_increment": "0.0001", "ask": "55000"}),
    ("MC-L02", "accumulate done when the remainder is below one increment (fees in the asset)", "btc_accumulator",
     {"now": at(15, 0), "position_qty": "0.14995", "goal_spent_usd": "8300", "min_order_usd": "1", "qty_increment": "0.0001", "ask": "55000"}),
    ("MC-L03", "accumulate spend exhausted", "btc_accumulator",
     {"now": at(15, 0), "position_qty": "0.14", "goal_spent_usd": "8999.5", "min_order_usd": "1", "qty_increment": "0.0001", "ask": "55000"}),
    ("MC-L04", "End date passes at 00:00 America/New_York after end_date", "btc_accumulator",
     {"now": "2027-01-01T05:00:00.000000000Z", "position_qty": "0.1", "goal_spent_usd": "5000", "min_order_usd": "1", "qty_increment": "0.0001", "ask": "55000"}),
    ("MC-L05", "Still on end_date (23:59 New York)", "btc_accumulator",
     {"now": "2027-01-01T04:59:00.000000000Z", "position_qty": "0.1", "goal_spent_usd": "5000", "min_order_usd": "1", "qty_increment": "0.0001", "ask": "55000"}),
]
for cid, title, base, st in GL:
    cases.append({"id": cid, "kind": "goal", "title": title, "base": base, "state": st, "expect": goal_status(MB[base], st)})

# =========================================================== K. change classification
PIN = [rep("/universe/pinned", True),
       rep("/universe/pinned_instruments", [{"asset_id": XYZ, "symbol": "XYZ", "asset_class": "us_equity"}]),
       rep("/universe/max_instruments", 1), rep("/behavior/research", None),
       rep("/behavior/signal_models/0/admits_instruments", False)]
UNPIN = [rep("/universe/pinned", False), rep("/universe/pinned_instruments", []),
         rep("/universe/max_instruments", 5), rep("/behavior/research", RESEARCH),
         rep("/behavior/signal_models/0/admits_instruments", True)]
derived("research_equity_pinned", "research_equity", PIN, "bring-your-own-strategy: the universe is pinned to XYZ")
derived("research_equity_no_agent", "research_equity",
        [rep("/behavior/research", None), rep("/behavior/signal_models/0/admits_instruments", False)],
        "unpinned with no research agent: the working universe stays empty")
derived("research_equity_two_classes", "research_equity",
        [rep("/universe/asset_classes", ["crypto", "us_equity"]), rep("/protection/crypto_stop_limit_offset", "0.005")],
        "adds crypto to the allowed asset classes")
derived("btc_accumulator_with_deny", "btc_accumulator",
        [{"op": "add", "path": "/autonomy/rules/2", "value": {"id": "deny_big_low", "when": {"field": "order_usd", "op": "gt", "value": "800"}, "then": "deny"}}],
        "adds rule deny_big_low at index 2")
CH = [
    ("MC-C01", "Raise max_drawdown (and the last rung)", "btc_accumulator",
     [rep("/risk/max_drawdown", "0.09"), rep("/risk/drawdown_ladder/2/at", "0.09")]),
    ("MC-C02", "Add an instrument", "two_stock_swing",
     [{"op": "add", "path": "/universe/pinned_instruments/-", "value": {"asset_id": LMN, "symbol": "LMN", "asset_class": "us_equity"}},
      rep("/universe/max_instruments", 3)]),
    ("MC-C03", "Remove an instrument", "two_stock_swing", [{"op": "remove", "path": "/universe/pinned_instruments/1"}]),
    ("MC-C04", "Raise the large-order ask threshold", "btc_accumulator", [rep("/autonomy/rules/0/when/value", "950")]),
    ("MC-C05", "Raise the low-score ask threshold", "btc_accumulator", [rep("/autonomy/rules/1/when/value", "0.7")]),
    ("MC-C06", "Lower max_daily_loss", "btc_accumulator", [rep("/risk/max_daily_loss", "0.01")]),
    ("MC-C07", "Add a notification channel", "btc_accumulator", [rep("/notifications/channels", ["email", "sms", "web_push"])]),
    ("MC-C08", "Remove a notification channel", "btc_accumulator", [rep("/notifications/channels", ["email"])]),
    ("MC-C09", "Change quiet hours", "btc_accumulator", [rep("/notifications/quiet_hours/start", "22:00")]),
    ("MC-C10", "Mixed reducing and increasing", "btc_accumulator", [rep("/risk/max_daily_loss", "0.01"), rep("/risk/max_orders_per_day", 60)]),
    ("MC-C11", "Make the routine rule stricter (auto to ask)", "btc_accumulator", [rep("/autonomy/rules/2/then", "ask")]),
    ("MC-C12", "Widen an ask rule that has a later deny rule", "btc_accumulator_with_deny", [rep("/autonomy/rules/1/when/value", "0.7")]),
    ("MC-C13", "Add a deny rule first", "btc_accumulator",
     [{"op": "add", "path": "/autonomy/rules/0", "value": {"id": "no_crypto", "when": {"field": "session", "op": "eq", "value": "crypto"}, "then": "deny"}}]),
    ("MC-C14", "Add an ask rule before a deny rule", "btc_accumulator_with_deny",
     [{"op": "add", "path": "/autonomy/rules/0", "value": {"id": "ask_all", "when": {"field": "order_usd", "op": "gt", "value": "0"}, "then": "ask"}}]),
    ("MC-C15", "Remove an ask rule (later rule is auto)", "btc_accumulator", [{"op": "remove", "path": "/autonomy/rules/1"}]),
    ("MC-C16", "Remove the auto rule (falls through to ask)", "btc_accumulator", [{"op": "remove", "path": "/autonomy/rules/2"}]),
    ("MC-C17", "Narrow a compound condition", "btc_accumulator",
     [rep("/autonomy/rules/0/when", {"all": [{"field": "order_usd", "op": "gt", "value": "900"}, {"field": "session", "op": "eq", "value": "crypto"}]})]),
    ("MC-C18", "Reorder rules", "btc_accumulator",
     [rep("/autonomy/rules", [RULES[1], RULES[0], RULES[2]])]),
    ("MC-C19", "Lower the entry threshold", "btc_accumulator", [rep("/behavior/sizing/entry_threshold", "0.2")]),
    ("MC-C20", "Raise the exit threshold (exits rarer)", "btc_accumulator", [rep("/behavior/sizing/exit_threshold", "0.4")]),
    ("MC-C21", "Change the cadence interval", "btc_accumulator", [rep("/behavior/cadence/interval_s", 1800)]),
    ("MC-C22", "Edit the description (fed to LLM models)", "btc_accumulator", [rep("/behavior/description", "Buy dips.")]),
    ("MC-C23", "Rename the agent", "btc_accumulator", [rep("/name", "btc-stacker")]),
    ("MC-C24", "Disable protection", "btc_accumulator", [rep("/protection", NO_PROT)]),
    ("MC-C25", "Change a signal-model parameter", "btc_accumulator", [rep("/behavior/signal_models/0/params/1/value", "2")]),
    ("MC-C26", "Raise the allocation", "btc_accumulator", [rep("/capital/allocation_usd", "12000")]),
    ("MC-C27", "Lower the allocation", "btc_accumulator",
     [rep("/capital/allocation_usd", "8000"), rep("/risk/max_position_usd", "8000"), rep("/risk/max_gross_exposure_usd", "8000")]),
    ("MC-C28", "Set a two-approver threshold", "btc_accumulator", [rep("/autonomy/approval/two_approver_above_usd", "700")]),
    ("MC-C29", "Raise the lifetime loss floor fraction", "btc_accumulator", [rep("/capital/max_loss_from_allocation", "0.2")]),
    ("MC-C30", "Shorten the re-entry cooldown", "btc_accumulator", [rep("/risk/reentry_cooldown_s", 600)]),
    ("MC-C31", "Lengthen breach confirmation", "btc_accumulator", [rep("/risk/breach_confirm_s", 120)]),
    ("MC-C32", "scale_action to trim_to_target", "btc_accumulator", [rep("/risk/scale_action", "trim_to_target")]),
    ("MC-C33", "Change on_complete", "btc_accumulator", [rep("/goal/on_complete", "release")]),
    ("MC-C34", "Change the environment", "btc_accumulator", [rep("/environment", "live")]),
    ("MC-C35", "Remove the end date (null means no end)", "btc_accumulator", [rep("/goal/end_date", None)]),
    ("MC-C36", "Pin the universe (bring-your-own-strategy on)", "research_equity", PIN),
    ("MC-C37", "Unpin the universe (the platform may admit again)", "research_equity_pinned", UNPIN),
    ("MC-C38", "Raise max_instruments", "research_equity", [rep("/universe/max_instruments", 8)]),
    ("MC-C39", "Lower max_instruments", "research_equity", [rep("/universe/max_instruments", 3)]),
    ("MC-C40", "Add an allowed asset class", "research_equity",
     [rep("/universe/asset_classes", ["crypto", "us_equity"]), rep("/protection/crypto_stop_limit_offset", "0.005")]),
    ("MC-C41", "Remove an allowed asset class", "research_equity_two_classes", [rep("/universe/asset_classes", ["us_equity"]),
                                                                               rep("/protection/crypto_stop_limit_offset", None)]),
    ("MC-C42", "Turn the research agent off (the signal-model row stays fail-safe)", "research_equity",
     [rep("/behavior/research", None), rep("/behavior/signal_models/0/admits_instruments", False)]),
    ("MC-C43", "Raise the revisions allowed per lineage", "research_equity", [rep("/behavior/research/max_revisions_per_lineage", 5)]),
    ("MC-C44", "Lower the research cost cap", "research_equity", [rep("/behavior/research/cost_cap_usd_per_day", "2")]),
    ("MC-C45", "Lengthen the research interval", "research_equity", [rep("/behavior/research/interval_s", 7200)]),
    ("MC-C46", "Make the admission ceiling stricter (ask to deny)", "research_equity", [rep("/autonomy/admission", "deny")]),
    ("MC-C47", "Loosen the admission ceiling (ask to auto)", "research_equity", [rep("/autonomy/admission", "auto")]),
    ("MC-C48", "Pin a mandate that had no research agent (it gains instruments it could not trade)",
     "research_equity_no_agent",
     [rep("/universe/pinned", True),
      rep("/universe/pinned_instruments", [{"asset_id": XYZ, "symbol": "XYZ", "asset_class": "us_equity"}]),
      rep("/universe/max_instruments", 1)]),
]
for cid, title, base, patch in CH:
    old = MB[base]
    new = apply_patch(old, patch)
    assert V.is_valid(new), (cid, [e.message for e in V.iter_errors(new)])
    got, paths = classify(old, new)
    e = {"classification": got, "changed_paths": paths, "old_version": version(old), "new_version": version(new)}
    if got != "invalid":
        e["step_up_required"] = got == "risk_increasing"
    cases.append({"id": cid, "kind": "change", "title": title, "base": base, "patch": patch, "expect": e})

# =========================================================== L. research agent: admission, lineage, expiry
TH_NOW = "2026-09-22T14:00:00.000000000Z"
SOURCES = ["src.filings", "src.newswire"]
ADM_ACTION = {"order_usd": "300", "combined_score": "0.8", "instrument": XYZ, "asset_class": "us_equity",
              "session": "regular", "first_trade_in_instrument": True, "drawdown": "0", "daily_pnl_fraction": "0",
              "position_usd_after": "300", "gross_usd_after": "300", "bought_today_usd": "300",
              "position_pnl_fraction": "0", "unusual_input": False}

def thesis(tid, inst=ABC, cls="us_equity", conv="0.7", conf="0.8", horizon=86400, rev=0, lineage=None, pred=None,
           direction="long", sources=None, corroboration="independent_source", as_of=TH_NOW, expires=None, etp=False):
    return {"thesis_id": tid, "lineage_id": lineage or tid, "revision": rev, "predecessor_thesis_id": pred,
            "instrument_id": inst, "asset_class": cls, "direction": direction, "horizon_s": horizon,
            "conviction": conv, "confidence": conf, "as_of": as_of,
            "expires_at": expires or fmt(T(as_of) + timedelta(seconds=horizon)),
            "evidence_sources": SOURCES if sources is None else sources,
            "corroboration": {"kind": corroboration} if corroboration else {},
            "leveraged_etp": etp, "invalidation": "Guidance is cut, or the 50-day trend breaks."}

ADM_BASE = {"working_universe": [], "eligibility_failures": [], "allowlisted_sources": SOURCES,
            "instrument_groups": {}, "claimed_by_other_agents": [], "halted_instruments": [],
            "data_universe": None, "research_spend_usd_today": "0", "lineages": {},
            "disclosures_accepted": [], "admission_action": ADM_ACTION}
N = [
    ("MC-N01", "A corroborated thesis in an allowed asset class is admitted", "research_equity", {"thesis": thesis("th-1")}),
    ("MC-N02", "max_instruments is full", "research_equity",
     {"thesis": thesis("th-2"), "working_universe": [XYZ, QRS, LMN, BTC, "7b4a1c2e-9999-4a2b-9c3d-000000000009"]}),
    ("MC-N03", "An asset class outside the envelope is refused", "research_equity", {"thesis": thesis("th-3", inst=BTC, cls="crypto")}),
    ("MC-N04", "A thesis failing the eligibility floor is refused", "research_equity",
     {"thesis": thesis("th-4"), "eligibility_failures": [ABC]}),
    ("MC-N05", "An instrument group claimed by another agent is refused", "research_equity",
     {"thesis": thesis("th-5"), "instrument_groups": {ABC: "grp_a", LMN: "grp_a"}, "claimed_by_other_agents": [LMN]}),
    ("MC-N06", "A thesis with no corroboration is refused (DEC-101)", "research_equity",
     {"thesis": thesis("th-6", corroboration=None)}),
    ("MC-N07", "Evidence from a source off the allowlist is refused (DEC-101)", "research_equity",
     {"thesis": thesis("th-7", sources=["src.filings", "src.anonymous_blog"])}),
    ("MC-N08", "A pinned universe admits nothing", "research_equity_pinned", {"thesis": thesis("th-8", inst=XYZ)}),
    ("MC-N09", "The research cost cap refuses new theses for the day (DEC-120)", "research_equity",
     {"thesis": thesis("th-9"), "research_spend_usd_today": "5"}),
    ("MC-N10", "An operator per-thesis halt refuses the admission (DEC-100)", "research_equity",
     {"thesis": thesis("th-10"), "halted_instruments": [ABC]}),
    ("MC-N11", "The thin slice admits only research-basket instruments (DEC-103)", "research_equity",
     {"thesis": thesis("th-11"), "data_universe": [XYZ, QRS]}),
    ("MC-N12", "A short thesis is ignored (long only in v1)", "research_equity", {"thesis": thesis("th-12", direction="short")}),
    ("MC-N13", "An expiry that disagrees with the horizon is ignored", "research_equity",
     {"thesis": thesis("th-13", expires="2026-09-30T14:00:00.000000000Z")}),
    ("MC-N14", "Renewing an active instrument adds no second entry", "research_equity",
     {"thesis": thesis("th-14", inst=XYZ), "working_universe": [XYZ]}),
    ("MC-N15", "Admission deny refuses the admission outright", "research_equity_admission_deny", {"thesis": thesis("th-15")}),
    ("MC-N16", "A leveraged ETP without the owner opt-in is refused", "research_equity", {"thesis": thesis("th-16", etp=True)}),
    ("MC-N25", "A leveraged ETP is refused while the accepted disclosure is a different version (V-005)",
     "research_equity_etp", {"thesis": thesis("th-17", etp=True), "disclosures_accepted": ["sha256:" + "c" * 64]}),
    ("MC-N26", "A leveraged ETP with the opt-in and the accepted disclosure is admitted", "research_equity_etp",
     {"thesis": thesis("th-18", etp=True), "disclosures_accepted": ["sha256:" + "b" * 64]}),
]
derived("research_equity_etp", "research_equity",
        [rep("/universe/leveraged_etps_enabled", True), rep("/universe/leveraged_etp_disclosure_version", "sha256:" + "b" * 64)],
        "leveraged ETPs enabled with an accepted disclosure version")
derived("research_equity_admission_deny", "research_equity", [rep("/autonomy/admission", "deny")], "autonomy.admission deny")
for cid, title, base, over in N:
    inp = dict(ADM_BASE, **over)
    cases.append({"id": cid, "kind": "admission", "title": title, "base": base, "patch": [], "input": inp,
                  "expect": admit(MB[base], inp)})

LIN = [
    ("MC-N17", "Revisions 1 to 3 are admitted; revision 4 retires the lineage (DEC-111)", "research_equity",
     [thesis("th-20"), thesis("th-21", rev=1, lineage="th-20", pred="th-20"),
      thesis("th-22", rev=2, lineage="th-20", pred="th-21"), thesis("th-23", rev=3, lineage="th-20", pred="th-22"),
      thesis("th-24", rev=4, lineage="th-20", pred="th-23")]),
    ("MC-N18", "A revision never carries its predecessor score forward (DEC-111)", "research_equity",
     [thesis("th-30"), thesis("th-31", rev=1, lineage="th-30", pred="th-30")]),
    ("MC-N19", "A revision without a predecessor id is ignored", "research_equity",
     [thesis("th-40", rev=1, lineage="th-40", pred=None)]),
    ("MC-N24", "Retiring a lineage removes the instrument it holds (DEC-111)", "research_equity_cap_one",
     [thesis("th-50"), thesis("th-51", rev=1, lineage="th-50", pred="th-50"),
      thesis("th-52", rev=2, lineage="th-50", pred="th-51")]),
    ("MC-N27", "An over-cap revision an earlier check refuses retires nothing", "research_equity_cap_one",
     [thesis("th-60"), thesis("th-61", rev=1, lineage="th-60", pred="th-60"),
      thesis("th-62", rev=2, lineage="th-60", pred="th-61", direction="short")]),
    ("MC-N28", "Retirement never removes an instrument another lineage now holds", "research_equity_cap_one",
     [thesis("th-70"), thesis("th-71", rev=1, lineage="th-70", pred="th-70"),
      thesis("th-80"), thesis("th-72", rev=2, lineage="th-70", pred="th-71")]),
]
derived("research_equity_cap_one", "research_equity", [rep("/behavior/research/max_revisions_per_lineage", 1)],
        "max_revisions_per_lineage 1")
for cid, title, base, theses in LIN:
    inp = dict(ADM_BASE, theses=theses)
    cases.append({"id": cid, "kind": "lineage", "title": title, "base": base, "patch": [], "input": inp,
                  "expect": lineage_fold(MB[base], inp)})

EX_ENTRY = {"instrument": ABC, "thesis_id": "th-1", "lineage_id": "th-1", "revision": 0,
            "expires_at": "2026-09-23T14:00:00.000000000Z"}
RETIRED = {"th-1": {"revisions": 3, "admitted": 4, "retired": True}}
EXP = [
    ("MC-N20", "A thesis at its horizon removes its instrument (DEC-118)", "2026-09-23T14:00:00.000000000Z",
     [dict(EX_ENTRY)], {}),
    ("MC-N21", "An invalidated thesis removes at once, before its horizon", "2026-09-22T18:00:00.000000000Z",
     [dict(EX_ENTRY, invalidated=True)], {}),
    ("MC-N22", "A retired lineage removes its instrument, an unexpired thesis stays", "2026-09-22T18:00:00.000000000Z",
     [dict(EX_ENTRY), dict(EX_ENTRY, instrument=XYZ, thesis_id="th-2", lineage_id="th-2")], RETIRED),
]
for cid, title, now, entries, lineages in EXP:
    inp = {"now": now, "entries": entries, "lineages": lineages}
    cases.append({"id": cid, "kind": "thesis_expiry", "title": title, "base": "research_equity", "input": inp,
                  "expect": thesis_expiry(research, inp)})

cases.append({"id": "MC-N23", "kind": "stagger",
              "title": "The stagger offset is deterministic per workspace and thesis, inside the window (DEC-100)",
              "input": {"window_s": 900, "pairs": [["ws_a", "th-1"], ["ws_b", "th-1"], ["ws_a", "th-2"]]},
              "expect": {"offsets": [stagger_offset("ws_a", "th-1", 900), stagger_offset("ws_b", "th-1", 900),
                                     stagger_offset("ws_a", "th-2", 900)],
                         "window_s": 900}})

# =========================================================== output
HEADER = """# Reference cases for docs/specs/mandate.md (spec v0.6)
#
# Generated by a reference implementation that is fuzzed against invariants MI-1 to MI-20
# (spec 1.1); every expected value is computed, not typed.
#
# HARNESS RULES
# Values
# - Decimals are strings and compare by decimal value. Timestamps use the journal form (journal
#   spec 4.7). Reported ratios are rounded half-even to 12 places (spec 5.2); combined conviction,
#   buy conviction, and combined score are rounded to 12 places before use (spec 8.3).
# - `base` names a mandate in `bases`; `patch` is an RFC 6902 JSON Patch applied to it first.
# Kinds
# - schema: validate against schemas/mandate.schema.json (JSON Schema 2020-12).
# - semantic: `context` overrides `validation_context_defaults`. `provenance` maps a JSON Pointer to
#   {source, confirmed}; absent paths are {user_entered, confirmed: true}. Expect the sorted V- and
#   W-codes and the worst-case figures shown on the confirmation screen (spec 4.2).
# - policy: `policies` are listed outermost first; each violation reports the key, the violating
#   level and value, and the nearest ancestor whose value it breaks (spec 4.3).
# - risk_state: one agent, one instrument, zero fees. `goal_complete` steps apply the mandate's
#   `on_complete`. Equity regular-session time is computed from
#   the calendar (09:30-16:00 America/New_York; the case dates are trading days). E = allocation + realized P&L +
#   qty x risk mark - cost basis. Each step's `at` is the risk clock at that input; `session` is the
#   session at that time; `clock` steps are copied ClockAdvanced ticks. The case starts at
#   `initial.at` with E = H = E0 = C = allocation and mode normal. `expect` is the state after the
#   step; `journal` lists the account-stream events emitted, in order; `pending` lists limits with
#   breach time accumulating; `harness_defaults.mark_max_age_s` is the staleness limit.
# - risk_day: the risk day containing `at` and its bounds (spec 5.4).
# - gate: only the mandate limits of spec 5.3, in trading spec 9.1 order; every other check passes.
#   `state.working_universe` is required: an instrument outside it is denied `not_in_working_universe`.
# - builder: the risk engine's trim (spec 5.5), the order builder (spec 8.3), the gate dry run, and
#   autonomy. `session` defaults to regular; `in_close_window` to false; fee rates to 0;
#   `has_prior_fill` to (position_qty > 0).
# - autonomy: evaluate spec 6.2 for the given action, including the admission ceiling of 6.2 step 5.
# - admission: the ordered spec 8.5 checks for one thesis; the first failure is the reason, and the
#   three shape reasons also set `ignored` (spec 8.2). `first_order_autonomy` is the decision for the
#   first order in an admitted instrument, null when the thesis is refused.
# - lineage: fold spec 8.6 over `input.theses` in order, carrying the working universe and the
#   lineage state; retiring a lineage removes its instrument (reason `lineage_retired`).
# - thesis_expiry: spec 8.6 at `input.now`; retirement is read from `input.lineages`, never per entry.
# - stagger: the deterministic offset of spec 8.4 for each [workspace_id, thesis_id] pair.
# - agent_flatten: the agent-scoped kill-switch plan (trading spec 5.5).
# - goal: goal completion (spec 3.1) for the given state.
# - change: classify `base` against the patched mandate (spec 9.2).
"""

class Dumper(yaml.SafeDumper):
    def ignore_aliases(self, data):
        return True
def str_rep(d, s):
    return d.represent_scalar("tag:yaml.org,2002:str", s, style="|" if len(s) > 120 else None)
Dumper.add_representer(str, str_rep)

if __name__ == "__main__":
    path = f"{ROOT}/docs/specs/reference-cases/mandate.yaml"
    open(path, "w").write(HEADER + "\n" + yaml.dump(doc, Dumper=Dumper, sort_keys=False, allow_unicode=True, width=110,
                                                    default_flow_style=None))
    back = yaml.safe_load(open(path))
    assert back == json.loads(json.dumps(doc))
    print(Counter(c["kind"] for c in cases), len(cases), version(btc))
