"""Generates docs/specs/reference-cases/mandate.yaml for mandate spec v0.7."""
import copy, json
from collections import Counter
import yaml
from ref import *  # noqa: F401,F403
from ref import D, ROOT
from bases import *  # noqa: F401,F403

doc = {"version": 4, "spec": "docs/specs/mandate.md (v0.7)",
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
def to_v2(offset):
    """Schema version 2 (DEC-539): `crypto_stop_limit_offset` becomes `stop_limit_offset`."""
    return [rep("/mandate_schema_version", 2), {"op": "remove", "path": "/protection/crypto_stop_limit_offset"},
            {"op": "add", "path": "/protection/stop_limit_offset", "value": offset}]
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
SEM += [
    ("MC-V69", "Independent approval required in a one-user workspace", "btc_accumulator", [],
     {"independent_approval_required": True, "workspace_users": 1}),
    ("MC-V70", "Independent approval required in a two-user workspace passes V-047", "btc_accumulator", [],
     {"independent_approval_required": True, "workspace_users": 2}),
    ("MC-V71", "A one-user workspace without independent approval is fine", "btc_accumulator", [],
     {"independent_approval_required": False, "workspace_users": 1}),
]
LONE_UNDER_POLICY = {"independent_approval_required": True, "workspace_users": 1}
FEWER_ORDERS = [rep("/risk/max_orders_per_day", 40)]
CURRENT = {"previous_version": MB["btc_accumulator"], "current_mandate_version": version(MB["btc_accumulator"])}
IDENTITY_ONLY = {"environment": "paper", "connection_id": "conn_alpaca_paper_01"}
OVER_SCHEMA = apply_patch(MB["btc_accumulator"], [rep("/risk/max_orders_per_day", 10001)])
assert not V.is_valid(OVER_SCHEMA)
FORGED = apply_patch(MB["btc_accumulator"], [rep("/risk/max_order_usd", "9000")])
SEM += [
    ("MC-V72", "A risk-reducing version in a one-user workspace under independent approval passes V-047 (DEC-444)",
     "btc_accumulator", FEWER_ORDERS, dict(LONE_UNDER_POLICY, **CURRENT)),
    ("MC-V73", "A neutral version in a one-user workspace under independent approval is refused", "btc_accumulator",
     [rep("/name", "btc-accumulator-renamed")], dict(LONE_UNDER_POLICY, **CURRENT)),
    ("MC-V74", "A version that reduces one limit and raises another, in a one-user workspace, is refused",
     "btc_accumulator", FEWER_ORDERS + [rep("/risk/max_order_usd", "1500")], dict(LONE_UNDER_POLICY, **CURRENT)),
    ("MC-V75", "A reducing version whose previous document validation lacks, in a one-user workspace, is refused",
     "btc_accumulator", FEWER_ORDERS,
     dict(LONE_UNDER_POLICY, previous_version=IDENTITY_ONLY, current_mandate_version=version(IDENTITY_ONLY))),
    ("MC-V76", "A raise against a forged previous document, not the agent's current version, is refused",
     "btc_accumulator", [rep("/risk/max_order_usd", "5000")],
     dict(LONE_UNDER_POLICY, previous_version=FORGED, current_mandate_version=version(MB["btc_accumulator"]))),
    ("MC-V77", "A reducing version against a previous document the schema refuses, matching its own hash, is refused",
     "btc_accumulator", FEWER_ORDERS,
     dict(LONE_UNDER_POLICY, previous_version=OVER_SCHEMA, current_mandate_version=version(OVER_SCHEMA))),
]
STOP_LIMIT_EQUITIES = {"stop_limit_asset_classes": ["crypto", "us_equity"]}
ALPACA_PROFILE = {"stop_limit_asset_classes": ["crypto"]}
PROFILE_ABSENT = {"stop_limit_asset_classes": None}
for cid, title, base, patch, ctx in SEM:
    m = apply_patch(MB[base], patch)
    assert V.is_valid(m), (cid, [e.message for e in V.iter_errors(m)])
    errs, warns = semantic(m, dict(CTX, **ctx))
    cases.append({"id": cid, "kind": "semantic", "title": title, "base": base, "patch": patch, "context": ctx,
                  "expect": {"violations": errs, "warnings": warns, "worst_case": worst_case(m, dict(CTX, **ctx))}})

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
    assert not semantic(m, CTX)[0], (cid, "a risk case's mandate passes every V-rule", semantic(m, CTX)[0])
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
          ISO + [rep("/risk/breach_confirm_s", 0)], "0.1", "60000", "crypto",
          at(14, 0), [mk(at(14, 1), "51600", "crypto"), mk(at(14, 2), "51500", "crypto")], L=RELEASE_CARRY,
          note="L is MC-R25's carry, 150. Floor: E <= C x (1 - 0.1) + L = 9000 + 150 = 9150. E = 9160 at 51600 latches the "
               "ladder but stays above the floor; E = 9150 at 51500 latches it, 150 above the floor a fresh connection would "
               "have (§5.7, V-032).")
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
     dict(BI, position_qty="10", size_factor="0.5", scale_active_s=120, min_order_size="1", open_sell_qty="0", gate_state=gst(positions_mv={XYZ: "999"}), outputs=TWO)),
    ("MC-B30", "trim_to_target withheld until the rung is confirmed", "two_stock_swing_trim",
     dict(BI, position_qty="10", size_factor="0.5", scale_active_s=10, min_order_size="1", open_sell_qty="0", gate_state=gst(positions_mv={XYZ: "999"}), outputs=TWO)),
    ("MC-B31", "trim_to_target withheld outside the regular session and while holding", "two_stock_swing_trim",
     at_now(dict(BI, position_qty="10", size_factor="0.5", scale_active_s=120, session="after_hours", holding=True,
                 min_order_size="1", open_sell_qty="0", gate_state=gst(positions_mv={XYZ: "999"}), outputs=TWO), AFTER_HOURS)),
    ("MC-B32", "No trim when the excess is below the rebalance band", "two_stock_swing_trim",
     dict(BI, position_qty="8", size_factor="0.5", scale_active_s=120, min_order_size="1", open_sell_qty="0", gate_state=gst(positions_mv={XYZ: "799.2"}), outputs=TWO)),
    ("MC-B33", "trim_to_target: a trim of at least the instrument's minimum size goes under a larger dollar minimum",
     "two_stock_swing_trim",
     dict(BI, position_qty="10", size_factor="0.5", scale_active_s=120, min_order_usd="500", min_order_size="3", open_sell_qty="0",
          gate_state=gst(positions_mv={XYZ: "999"}), outputs=TWO)),
    ("MC-B34", "trim_to_target withheld below the instrument's minimum order size", "two_stock_swing_trim",
     dict(BI, position_qty="10", size_factor="0.5", scale_active_s=120, min_order_size="4", open_sell_qty="0",
          gate_state=gst(positions_mv={XYZ: "999"}), outputs=TWO)),
    ("MC-B35", "trim_to_target: a trim of the whole position goes below the instrument's minimum order size",
     "two_stock_swing_trim",
     dict(BI, position_qty="1", quote={"bid": "999", "ask": "1000"}, size_factor="0.5", scale_active_s=120,
          min_order_size="2", open_sell_qty="0", gate_state=gst(positions_mv={XYZ: "999"}), outputs=TWO)),
    ("MC-B36", "trim_to_target: a resting sell leaves a remainder below the minimum size, which is withheld",
     "two_stock_swing_trim",
     dict(BI, position_qty="10", size_factor="0.5", scale_active_s=120, min_order_size="3", open_sell_qty="2",
          gate_state=gst(positions_mv={XYZ: "999"}), outputs=TWO)),
    ("MC-B37", "trim_to_target: a resting sell leaves a remainder, which is the trim",
     "two_stock_swing_trim",
     dict(BI, position_qty="10", size_factor="0.5", scale_active_s=120, min_order_size="1", open_sell_qty="2",
          gate_state=gst(positions_mv={XYZ: "999"}), outputs=TWO)),
    ("MC-B38", "trim_to_target: a remainder beside a resting sell that is off the grid is truncated onto it",
     "two_stock_swing_trim",
     dict(BI, position_qty="5", quote={"bid": "999", "ask": "1000"}, size_factor="0.5", scale_active_s=120,
          min_order_size="2", qty_increment="2", open_sell_qty="2", gate_state=gst(positions_mv={XYZ: "4995"}),
          outputs=TWO)),
    ("MC-B39", "trim_to_target: the resting sells come off the excess before the trim rounds up on the grid",
     "two_stock_swing_trim",
     dict(BI, position_qty="10", size_factor="0.5", scale_active_s=120, min_order_size="2", qty_increment="2",
          open_sell_qty="1", gate_state=gst(positions_mv={XYZ: "999"}), outputs=TWO)),
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
derived("btc_accumulator_v2", "btc_accumulator", to_v2("0.005"), "schema version 2 (DEC-539)")
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

# =========================================================== K2. one stop-limit offset (DEC-539)
# Family K is its own until the code reads schema version 2: families S, V and C are counted and must all pass.
K = [
    ("MC-K01", "schema", "Version 2 names the offset stop_limit_offset", "btc_accumulator", to_v2("0.005"), {}),
    ("MC-K02", "schema", "Version 2 with version 1's field name", "btc_accumulator", [rep("/mandate_schema_version", 2)], {}),
    ("MC-K03", "schema", "Version 1 with version 2's field name", "btc_accumulator", to_v2("0.005")[1:], {}),
    ("MC-K04", "semantic", "Version 2: crypto with protection but no stop-limit offset", "btc_accumulator", to_v2(None), {}),
    ("MC-K05", "semantic", "Equities on a profile that protects them with a stop-limit need the offset",
     "two_stock_swing", to_v2(None), STOP_LIMIT_EQUITIES),
    ("MC-K06", "semantic", "With the offset set, it passes and the worst case adds it", "two_stock_swing", to_v2("0.01"),
     STOP_LIMIT_EQUITIES),
    ("MC-K07", "semantic", "An equity mandate on a profile with OCO and bracket passes without an offset", "two_stock_swing",
     to_v2(None), ALPACA_PROFILE),
    ("MC-K08", "semantic", "Version 1's crypto_stop_limit_offset is read as the offset, so it passes", "two_stock_swing",
     [rep("/protection/crypto_stop_limit_offset", "0.01")], STOP_LIMIT_EQUITIES),
    ("MC-K09", "change", "Raise the stop-limit offset", "btc_accumulator_v2", [rep("/protection/stop_limit_offset", "0.01")], {}),
    ("MC-K10", "change", "Move to version 2 with a smaller offset (version 1 reads as version 2)", "btc_accumulator",
     to_v2("0.004"), {}),
    ("MC-K11", "change", "Move to version 2 with the same offset", "btc_accumulator", to_v2("0.005"), {}),
    ("MC-K12", "semantic", "Profile absent: an equity mandate without an offset fails V-008 (fails closed)",
     "two_stock_swing", to_v2(None), PROFILE_ABSENT),
    ("MC-K13", "semantic", "Profile absent: an equity mandate with the offset passes, but the worst case adds it and W-002 fires",
     "two_stock_swing", to_v2("0.09"), PROFILE_ABSENT),
    ("MC-K14", "semantic", "On the Alpaca profile the same equity mandate passes, its offset out of the worst case, with no W-002",
     "two_stock_swing", to_v2("0.09"), ALPACA_PROFILE),
    ("MC-K15", "semantic", "A version-1 document after a version-2 previous version is refused (V-031)",
     "two_stock_swing", [], {"previous_version": apply_patch(MB["two_stock_swing"], to_v2(None))}),
]
for cid, kind, title, base, patch, ctx in K:
    old = MB[base]
    m = apply_patch(old, patch)
    if kind == "schema":
        cases.append({"id": cid, "kind": kind, "title": title, "base": base, "patch": patch,
                      "expect": {"schema_valid": V.is_valid(m)}})
        continue
    assert V.is_valid(m), (cid, [e.message for e in V.iter_errors(m)])
    if kind == "semantic":
        errs, warns = semantic(m, dict(CTX, **ctx))
        cases.append({"id": cid, "kind": kind, "title": title, "base": base, "patch": patch, "context": ctx,
                      "expect": {"violations": errs, "warnings": warns, "worst_case": worst_case(m, dict(CTX, **ctx))}})
    else:
        got, paths = classify(old, m)
        cases.append({"id": cid, "kind": kind, "title": title, "base": base, "patch": patch,
                      "expect": {"classification": got, "changed_paths": paths, "old_version": version(old),
                                 "new_version": version(m), "step_up_required": got == "risk_increasing"}})

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

# =========================================================== M. the review date (§6.2 step 5b, MI-32, V-046; DEC-188)
# The validation date is CTX's 2026-09-24, so the platform default is 2026-12-23 and the latest date V-046 allows
# is 2027-03-23. 2026-12-23 is in Eastern Standard Time: its last instant is 2026-12-24T04:59:59Z.
REVIEW_BY = "2026-12-23"
derived("btc_accumulator_reviewed", "btc_accumulator", [{"op": "add", "path": "/autonomy/review_by", "value": REVIEW_BY}],
        "adds a review date, the platform default for the validation date 2026-09-24")
LAST_INSTANT = "2026-12-24T04:59:59.000000000Z"
PASSED = "2026-12-24T05:00:00.000000000Z"
REVIEW_DELEGATION = {"id": "d1", "lifts": "rule:large_orders", "when": {"field": "purpose", "op": "in", "value": ["increase", "open"]},
                     "max_order_usd": "1000", "max_orders": 3, "max_total_usd": "3000",
                     "starts_at": "2026-12-20T00:00:00.000000000Z", "expires_at": "2027-01-10T00:00:00.000000000Z",
                     "source_approval_id": None}
with_review_delegation = apply_patch(MB["btc_accumulator_reviewed"], [{"op": "add", "path": "/autonomy/delegations", "value": [REVIEW_DELEGATION]}])
SEM_D = [
    ("MC-D01", "A review date at the platform default is valid", [{"op": "add", "path": "/autonomy/review_by", "value": REVIEW_BY}],
     {"provenance": {"/autonomy/review_by": PU("platform_default")}}),
    ("MC-D02", "A platform-default review date other than 90 days after validation is refused",
     [{"op": "add", "path": "/autonomy/review_by", "value": "2027-01-22"}], {"provenance": {"/autonomy/review_by": PU("platform_default")}}),
    ("MC-D03", "A review date on the validation date is valid", [{"op": "add", "path": "/autonomy/review_by", "value": "2026-09-24"}], {}),
    ("MC-D04", "A review date 180 days after validation is valid", [{"op": "add", "path": "/autonomy/review_by", "value": "2027-03-23"}], {}),
    ("MC-D05", "A review date 181 days after validation is refused", [{"op": "add", "path": "/autonomy/review_by", "value": "2027-03-24"}], {}),
    ("MC-D06", "A new review date before the validation date is refused", [{"op": "add", "path": "/autonomy/review_by", "value": "2026-09-23"}], {}),
    ("MC-D07", "A review date that is not a calendar date is refused", [{"op": "add", "path": "/autonomy/review_by", "value": "2026-11-31"}], {}),
    ("MC-D08", "A lapsed review date carried unchanged stays valid", [{"op": "add", "path": "/autonomy/review_by", "value": "2026-06-01"}],
     {"previous_version": {"environment": "paper", "connection_id": "conn_alpaca_paper_01", "autonomy": {"review_by": "2026-06-01"}}}),
    ("MC-D09", "A version cannot remove a review date its previous version set", [],
     {"previous_version": {"environment": "paper", "connection_id": "conn_alpaca_paper_01", "autonomy": {"review_by": "2026-06-01"}}}),
    ("MC-D10", "A lapsed review date moved but still before validation is refused",
     [{"op": "add", "path": "/autonomy/review_by", "value": "2026-05-01"}],
     {"previous_version": {"environment": "paper", "connection_id": "conn_alpaca_paper_01", "autonomy": {"review_by": "2026-06-01"}}}),
]
for cid, title, patch, ctx in SEM_D:
    m = apply_patch(MB["btc_accumulator"], patch)
    assert V.is_valid(m), (cid, [e.message for e in V.iter_errors(m)])
    errs, warns = semantic(m, dict(CTX, **ctx))
    cases.append({"id": cid, "kind": "semantic", "title": title, "base": "btc_accumulator", "patch": patch, "context": ctx,
                  "expect": {"violations": errs, "warnings": warns, "worst_case": worst_case(m, dict(CTX, **ctx))}})
RECONFIRM = [{"op": "add", "path": "/autonomy/delegations", "value": [REVIEW_DELEGATION]}, rep("/autonomy/review_by", "2027-03-01")]
for cid, title, extra in [
    ("MC-D11", "Re-confirming moves the review date and carries the delegation over", []),
    ("MC-D12", "A re-confirmation that also raises a limit carries no delegation over", [rep("/risk/max_daily_loss", "0.03")]),
]:
    patch = RECONFIRM + extra
    m = apply_patch(MB["btc_accumulator_reviewed"], patch)
    assert V.is_valid(m), cid
    ctx = {"previous_version": with_review_delegation}
    errs, warns = semantic(m, dict(CTX, **ctx))
    cases.append({"id": cid, "kind": "semantic", "title": title, "base": "btc_accumulator_reviewed", "patch": patch, "context": ctx,
                  "expect": {"violations": errs, "warnings": warns, "worst_case": worst_case(m, dict(CTX, **ctx))}})
CH_D = [
    ("MC-D13", "Set a review date where there was none", "btc_accumulator", [{"op": "add", "path": "/autonomy/review_by", "value": REVIEW_BY}]),
    ("MC-D14", "Move the review date earlier", "btc_accumulator_reviewed", [rep("/autonomy/review_by", "2026-11-01")]),
    ("MC-D15", "Move the review date later (re-confirming)", "btc_accumulator_reviewed", [rep("/autonomy/review_by", "2027-03-01")]),
    ("MC-D16", "Remove the review date", "btc_accumulator_reviewed", [{"op": "remove", "path": "/autonomy/review_by"}]),
]
for cid, title, base, patch in CH_D:
    old = MB[base]
    new = apply_patch(old, patch)
    assert V.is_valid(new), cid
    got, paths = classify(old, new)
    cases.append({"id": cid, "kind": "change", "title": title, "base": base, "patch": patch,
                  "expect": {"classification": got, "changed_paths": paths, "old_version": version(old), "new_version": version(new),
                             "step_up_required": got == "risk_increasing"}})
OPEN = {"purpose": "open", "order_usd": "300", "combined_score": "0.8", "instrument": BTC, "asset_class": "crypto",
        "session": "crypto", "first_trade_in_instrument": False, "drawdown": "0", "daily_pnl_fraction": "0",
        "position_usd_after": "0", "gross_usd_after": "0", "bought_today_usd": "0", "position_pnl_fraction": "0",
        "new_instrument": False, "thesis_confidence": "0"}
DENY_FIRST = {"op": "add", "path": "/autonomy/rules/0",
              "value": {"id": "deny_big", "when": {"field": "order_usd", "op": "gt", "value": "800"}, "then": "deny"}}
REVIEW_A = [
    ("MC-D17", "On the review date's last instant an auto rule still decides", "btc_accumulator_reviewed", [], LAST_INSTANT, {}),
    ("MC-D18", "At 00:00 New York after the review date an auto rule asks", "btc_accumulator_reviewed", [], PASSED, {}),
    ("MC-D19", "Past the review date an ask keeps the rule that asked", "btc_accumulator_reviewed", [], PASSED,
     {"order_usd": "950", "combined_score": "0.9"}),
    ("MC-D20", "Past the review date a deny rule still denies", "btc_accumulator_reviewed", [DENY_FIRST], PASSED,
     {"order_usd": "950", "combined_score": "0.9"}),
    ("MC-D21", "Past the review date an auto default asks", "btc_accumulator_reviewed", [rep("/autonomy/rules", []), rep("/autonomy/default", "auto")],
     PASSED, {}),
    ("MC-D22", "Past the review date a new instrument's first order asks, though the admission setting is auto", "btc_accumulator_reviewed", [rep("/autonomy/admission", "auto")],
     PASSED, {"new_instrument": True, "thesis_confidence": "0.9"}),
    ("MC-D23", "Before the review date a live delegation lifts an ask", "btc_accumulator_reviewed",
     [{"op": "add", "path": "/autonomy/delegations", "value": [REVIEW_DELEGATION]}], LAST_INSTANT, {"order_usd": "950", "combined_score": "0.9"}),
    ("MC-D24", "Past the review date the same delegation lifts nothing", "btc_accumulator_reviewed",
     [{"op": "add", "path": "/autonomy/delegations", "value": [REVIEW_DELEGATION]}], PASSED, {"order_usd": "950", "combined_score": "0.9"}),
    ("MC-D25", "Past the review date a discretionary exit is still AUTO", "btc_accumulator_reviewed", [], PASSED, {"purpose": "discretionary_exit"}),
    ("MC-D26", "Past the review date an owner exit is still AUTO", "btc_accumulator_reviewed", [], PASSED, {"purpose": "owner_exit"}),
    ("MC-D27", "With no review date an auto rule decides at any time", "btc_accumulator", [], "2027-09-24T14:00:00.000000000Z", {}),
]
for cid, title, base, patch, now, a in REVIEW_A:
    m = apply_patch(MB[base], patch)
    V.validate(m)
    full = dict(OPEN, **a)
    if full["purpose"] in REDUCING:
        full = {"purpose": full["purpose"]}
    e = autonomy(m, full, {"now": now})
    if e["decision"] == "ask":
        e["trigger"] = approval_trigger(m, {"mandate_version": version(m), "decided_by": e["by"], "requested_by": "agent"})
    cases.append({"id": cid, "kind": "review", "title": title, "base": base, "patch": patch, "now": now, "action": full, "expect": e})

# =========================================================== N. escalation (§6.1, §6.4; MI-21 to MI-25; DEC-155, DEC-156, DEC-158, DEC-173)
E_T0 = "2026-09-22T14:00:00.000000000Z"
E_CTX = {"approvers": ["u1", "u2"], "author": "u1", "environment": "paper", "timeout_s": 300,
         "inbox": 1, "push_channels": [], "quiet_hours": None}
E_BOUND = {"instrument": ABC, "asset_class": "us_equity", "side": "buy", "qty": "10", "limit_price": "155",
           "purpose": "open", "mandate_version": "sha256:v1", "decided_by": "rule:big_order", "combined_score": "0.7",
           "reference_mark": {"price": "155", "seq": 2}, "approvers_required": 1, "independent_required": False,
           "timeout_s": 300}
E_NOW = {"mandate_version": "sha256:v1", "mode": "normal", "instrument_restricted": False, "in_working_universe": True,
         "classification": {"decision": "ask", "by": "rule:big_order"}, "dry_run": {"verdict": "allow", "reason": None},
         "mark": "155"}

def e_at(s, t0=E_T0):
    return fmt(T(t0) + timedelta(seconds=s))

def e_run(ctx, start, script):
    """The case's expectation: each input's drafts, as `escalation_step` gives them, folded before the next input.
    A `tick` is preceded by the scheduler's `ClockAdvanced`, and a `fold` input is an event folded with no step."""
    st = escalation_fold([], start)
    out = []
    for inp in script:
        if inp["kind"] == "fold":
            escalation_apply(st, inp["event"])
            out.append({"drafts": []})
            continue
        if inp["kind"] == "tick":
            escalation_apply(st, {"type": "ClockAdvanced", "clock": inp["at"]})
        drafts = escalation_step(st, inp, ctx)
        for d in drafts:
            escalation_apply(st, d)
        out.append({"drafts": json.loads(json.dumps(drafts, default=sorted))})
    return out

def e_hash(ctx, start, bound):
    asked = e_run(ctx, start, [{"kind": "ask", "approval": "ap1", "bound": bound}])
    return asked[0]["drafts"][0]["content_hash"]

def e_resp(src, at, hash_, t0=E_T0, **kw):
    r = {"source": src, "approval": "ap1", "actor_kind": "user", "responder": "u2", "verdict": "approved",
         "content_hash": hash_, "submitted_at": e_at(at, t0),
         "step_up": {"assertion": f"as-{src}", "authenticated_at": e_at(at, t0), "method": "cli_confirm"}}
    return r | kw

def e_case(cid, title, script, ctx=E_CTX, start=E_T0):
    cases.append({"id": cid, "kind": "escalation", "op": "lifecycle", "title": title, "context": ctx, "start": start,
                  "script": script, "expect": e_run(ctx, start, script)})

def e_grant_case(cid, title, bound=E_BOUND, now=E_NOW, ctx=E_CTX, start=E_T0, pre=(), **resp):
    h = e_hash(ctx, start, bound)
    ask = {"kind": "ask", "approval": "ap1", "bound": bound}
    r = e_resp("ctl1", 30, h, start) | resp
    e_case(cid, title, [ask, *pre, {"kind": "response", "response": r, "now": now}], ctx, start)

H1 = e_hash(E_CTX, E_T0, E_BOUND)
ASK1 = {"kind": "ask", "approval": "ap1", "bound": E_BOUND}
e_grant_case("MC-E01", "A timely admitted grant acts with exactly the bound order")
e_grant_case("MC-E02", "An admitted skip ends the approval, needs no step-up, and sends nothing", verdict="skipped", step_up=None)
e_case("MC-E03", "The deadline skips: ApprovalTimedOut, and a grant read after it is not pending",
       [ASK1, {"kind": "tick", "at": e_at(300)}, {"kind": "response", "response": e_resp("ctl1", 290, H1), "now": E_NOW}])
e_case("MC-E04", "A response submitted exactly at the deadline is late",
       [ASK1, {"kind": "response", "response": e_resp("ctl1", 300, H1), "now": E_NOW}])
e_case("MC-E05", "Late by the folded clock: an early submitted_at read once the clock reached the deadline is late",
       [ASK1, {"kind": "fold", "event": {"type": "MarkUpdated", "clock": e_at(300)}},
        {"kind": "response", "response": e_resp("ctl1", 100, H1), "now": E_NOW}])
e_case("MC-E06", "A duplicate response, re-tailed with the same source, is copied and acts once",
       [ASK1, {"kind": "response", "response": e_resp("ctl1", 30, H1), "now": E_NOW},
        {"kind": "response", "response": e_resp("ctl1", 30, H1), "now": E_NOW}])
e_grant_case("MC-E07", "A wrong content hash is refused as content_mismatch", content_hash="sha256:" + "f" * 64)
e_case("MC-E08", "A response to a cancelled approval is not pending",
       [ASK1, {"kind": "cancel", "reason": "version_applied"}, {"kind": "response", "response": e_resp("ctl1", 30, H1), "now": E_NOW}])
e_case("MC-E09", "A response from an agent or a system actor is refused as not_an_approver",
       [ASK1, {"kind": "response", "response": e_resp("ctl1", 30, H1, actor_kind="agent"), "now": E_NOW},
        {"kind": "response", "response": e_resp("ctl2", 31, H1, actor_kind="system"), "now": E_NOW}])
e_grant_case("MC-E10", "A response from a broker actor is refused as not_an_approver", actor_kind="broker")
e_grant_case("MC-E11", "A response from a platform_operator actor is refused as not_an_approver", actor_kind="platform_operator")
e_grant_case("MC-E12", "A user not in approvers is refused as not_an_approver", responder="u3")
e_grant_case("MC-E13", "A grant with no step-up evidence is refused as step_up_missing", step_up=None)
e_grant_case("MC-E14", "Step-up evidence 301 s old at the effective time is refused as step_up_stale",
             step_up={"assertion": "as-stale", "authenticated_at": e_at(30 - 301), "method": "cli_confirm"})
e_case("MC-E15", "A reused step-up assertion is refused as step_up_reused",
       [ASK1, {"kind": "response", "response": e_resp("ctl1", 30, "sha256:" + "f" * 64), "now": E_NOW},
        {"kind": "response", "response": e_resp("ctl2", 31, H1) | {"step_up": {"assertion": "as-ctl1",
         "authenticated_at": e_at(31), "method": "cli_confirm"}}, "now": E_NOW}])
E_LIVE = dict(E_CTX, environment="live")
e_grant_case("MC-E16", "cli_confirm on a live connection is refused as step_up_method", ctx=E_LIVE)
e_grant_case("MC-E17", "Re-validation skips a grant whose mandate version changed (version_changed)",
             now=dict(E_NOW, mandate_version="sha256:v2"))
e_grant_case("MC-E18", "An exits_only step cancels a pending grant first (mode_tightened), so it is refused as not pending and never acts",
             now=dict(E_NOW, mode="exits_only"))
e_grant_case("MC-E19", "Re-validation skips a grant re-classified deny (reclassified_deny)",
             now=dict(E_NOW, classification={"decision": "deny", "by": "rule:no_more"}))
e_grant_case("MC-E20", "Re-validation skips a grant re-classified ask by another rule (reclassified_other_trigger)",
             now=dict(E_NOW, classification={"decision": "ask", "by": "rule:other"}))
e_grant_case("MC-E21", "Drift exactly at the 100 bp equity band acts", now=dict(E_NOW, mark="156.55"))
e_grant_case("MC-E22", "Drift one unit beyond the band on a falling price skips (drift)", now=dict(E_NOW, mark="153.449999999"))
e_grant_case("MC-E23", "A request with no reference mark skips on re-validation (drift)", bound=dict(E_BOUND, reference_mark=None))
E_BTC = dict(E_BOUND, instrument=BTC, asset_class="crypto", qty="0.01", limit_price="50000",
             reference_mark={"price": "50000", "seq": 7})
e_grant_case("MC-E24", "Crypto drifts within its 200 bp band: a 150 bp move acts, where the equity band would skip",
             bound=E_BTC, now=dict(E_NOW, mark="50750"))

def e_permit(cid, title, ledger, queries):
    cases.append({"id": cid, "kind": "escalation", "op": "ask_permit", "title": title, "ledger": ledger,
                  "queries": queries, "expect": [{"suppressed": ask_permit(ledger, q["instrument"], q["at"])} for q in queries]})

TEN = [{"event": "requested", "instrument": ABC, "at": e_at(60 * i)} for i in range(10)]
e_permit("MC-E25", "The eleventh ask in a risk day is suppressed (budget), and the next risk day asks again",
         TEN, [{"instrument": ABC, "at": e_at(601)}, {"instrument": ABC, "at": "2026-09-23T14:00:00.000000000Z"}])
DST = "2026-11-01T03:00:00.000000000Z"
e_permit("MC-E26", "The ask budget resets at 00:00 America/New_York across the DST change, not at UTC midnight",
         [{"event": "requested", "instrument": ABC, "at": e_at(60 * i, DST)} for i in range(10)],
         [{"instrument": ABC, "at": "2026-11-01T03:59:59.000000000Z"}, {"instrument": ABC, "at": "2026-11-01T04:00:00.000000000Z"}])
e_permit("MC-E27", "An owner skip suppresses re-asking that instrument until the next risk day",
         [{"event": "requested", "instrument": ABC, "at": e_at(0)}, {"event": "owner_skipped", "instrument": ABC, "at": e_at(10)}],
         [{"instrument": ABC, "at": e_at(3600)}, {"instrument": BTC, "at": e_at(3600)},
          {"instrument": ABC, "at": "2026-09-23T14:00:00.000000000Z"}])
e_permit("MC-E28", "A timeout suppresses re-asking that instrument for one timeout_s",
         [{"event": "requested", "instrument": ABC, "at": e_at(0)},
          {"event": "timed_out", "instrument": ABC, "at": e_at(300), "timeout_s": 300}],
         [{"instrument": ABC, "at": e_at(599)}, {"instrument": ABC, "at": e_at(600)}])
QH = {"start": "23:00", "end": "07:00"}
e_grant_case("MC-E29", "A request at 02:00 New York inside quiet hours is delivered to cli_inbox and grantable",
             ctx=dict(E_CTX, quiet_hours=QH), start="2026-09-22T06:00:00.000000000Z")
E30Q = [{"channel": c, "at": a} for c, a in [
    ("push", "2026-07-16T03:00:00.000000000Z"), ("push", "2026-07-16T11:00:00.000000000Z"),
    ("push", "2026-12-16T04:00:00.000000000Z"), ("push", "2026-12-16T12:00:00.000000000Z"),
    ("cli_inbox", "2026-12-16T04:00:00.000000000Z")]]
cases.append({"id": "MC-E30", "kind": "escalation", "op": "deliver_now",
              "title": "A push is suppressed at 23:00 and sent at 07:00 New York in both DST states; cli_inbox always delivers",
              "quiet_hours": QH, "queries": E30Q,
              "expect": [{"status": deliver_now(q["channel"], QH, q["at"])} for q in E30Q]})
E32Q = [{"channel": "push", "at": a} for a in [
    "2026-07-16T23:30:00.000000000Z", "2026-12-16T23:30:00.000000000Z",
    "2026-07-16T10:30:00.000000000Z", "2026-12-16T11:30:00.000000000Z"]]
cases.append({"id": "MC-E32", "kind": "escalation", "op": "deliver_now",
              "title": "Quiet hours are New York wall time, not UTC: 23:30 UTC sends and 06:30 New York suppresses, in both DST states",
              "quiet_hours": QH, "queries": E32Q,
              "expect": [{"status": deliver_now(q["channel"], QH, q["at"])} for q in E32Q]})
e_case("MC-E31", "A grant batched with a cancelling exits-only restriction is not pending and never acts (DEC-131 item 25(j))",
       [ASK1, {"kind": "batch", "reason": "mode_tightened", "responses": [e_resp("ctl1", 30, H1)], "now": E_NOW}])

# =========================================================== P. delegation routing (§9.2, MI-11, MI-29; #444, DEC-353)
# A rule change the autonomy row would call reducing is increasing when it sends an order that reached an undelegated
# ask to an ask a delegation of the new version lifts (MI-29). An order that was auto stays auto, so removing or
# narrowing an auto rule ahead of a delegated ask stays reducing. Each delegated case has a control without the delegation.
J_DELEG = lambda lifts: {"op": "add", "path": "/autonomy/delegations", "value": [dict(REVIEW_DELEGATION, lifts=lifts)]}
J_TWO_ASKS = [rep("/autonomy/rules", [{"id": "large_orders", "when": {"field": "order_usd", "op": "gt", "value": "900"}, "then": "ask"},
                                     {"id": "low_score", "when": {"field": "combined_score", "op": "lt", "value": "0.65"}, "then": "ask"}])]
J_ONE_ASK = [rep("/autonomy/rules", [{"id": "large_orders", "when": {"field": "order_usd", "op": "gt", "value": "900"}, "then": "ask"}])]
derived("btc_accumulator_two_asks", "btc_accumulator", J_TWO_ASKS, "two ask rules and an ask default, no auto rule")
derived("btc_accumulator_two_asks_delegated", "btc_accumulator_two_asks", [J_DELEG("rule:low_score")],
        "a delegation lifts the later ask rule, low_score")
derived("btc_accumulator_one_ask_delegated_default", "btc_accumulator", J_ONE_ASK + [J_DELEG("default")],
        "one ask rule, and a delegation lifts the ask default")
derived("btc_accumulator_delegated_large", "btc_accumulator", [J_DELEG("rule:large_orders")],
        "a delegation lifts the ask rule large_orders")
J_SMALL = [{"op": "add", "path": "/autonomy/rules/0", "value": {"id": "small", "when": {"field": "order_usd", "op": "lt", "value": "100"}, "then": "auto"}}]
derived("btc_accumulator_two_asks_delegated_small", "btc_accumulator_two_asks_delegated", J_SMALL,
        "adds an auto rule, small, ahead of both asks")
derived("btc_accumulator_two_asks_small", "btc_accumulator_two_asks", J_SMALL, "adds an auto rule, small, ahead of both asks")
derived("btc_accumulator_one_ask", "btc_accumulator", J_ONE_ASK, "one ask rule and an ask default")
CH_J = [
    ("MC-J01", "Widening an ask rule a delegation lifts is risk-increasing", "btc_accumulator_delegated_large",
     [rep("/autonomy/rules/0/when/value", "800")]),
    ("MC-J02", "Widening the same ask rule with no delegation is risk-reducing", "btc_accumulator",
     [rep("/autonomy/rules/0/when/value", "800")]),
    ("MC-J03", "Removing an ask rule ahead of an ask rule a delegation lifts is risk-increasing", "btc_accumulator_two_asks_delegated",
     [{"op": "remove", "path": "/autonomy/rules/0"}]),
    ("MC-J04", "Removing the same ask rule with no delegation is risk-reducing", "btc_accumulator_two_asks",
     [{"op": "remove", "path": "/autonomy/rules/0"}]),
    ("MC-J05", "Removing an ask rule ahead of an ask default a delegation lifts is risk-increasing", "btc_accumulator_one_ask_delegated_default",
     [{"op": "remove", "path": "/autonomy/rules/0"}]),
    ("MC-J06", "Removing an auto rule ahead of an ask rule a delegation lifts stays risk-reducing: its orders stay auto",
     "btc_accumulator_two_asks_delegated_small", [{"op": "remove", "path": "/autonomy/rules/0"}]),
    ("MC-J07", "Removing the same auto rule with no delegation is risk-reducing", "btc_accumulator_two_asks_small",
     [{"op": "remove", "path": "/autonomy/rules/0"}]),
    ("MC-J08", "Removing the same ask rule ahead of the ask default with no delegation is risk-reducing", "btc_accumulator_one_ask",
     [{"op": "remove", "path": "/autonomy/rules/0"}]),
    ("MC-J09", "Narrowing an auto rule ahead of an ask rule a delegation lifts stays risk-reducing: its orders stay auto", "btc_accumulator_two_asks_delegated_small",
     [rep("/autonomy/rules/0/when/value", "50")]),
    ("MC-J10", "Narrowing the same auto rule with no delegation is risk-reducing", "btc_accumulator_two_asks_small",
     [rep("/autonomy/rules/0/when/value", "50")]),
]
for cid, title, base, patch in CH_J:
    old = MB[base]
    new = apply_patch(old, patch)
    assert V.is_valid(new) and semantic(old, CTX)[0] == [] and semantic(new, CTX)[0] == [], (cid, semantic(old, CTX), semantic(new, CTX))
    got, paths = classify(old, new)
    cases.append({"id": cid, "kind": "change", "title": title, "base": base, "patch": patch,
                  "expect": {"classification": got, "changed_paths": paths, "old_version": version(old), "new_version": version(new),
                             "step_up_required": got == "risk_increasing"}})

# =========================================================== O. tripwires (§6.7, MI-31, V-044; DEC-187, DEC-350 to DEC-352)
# Family W. The fold's inputs are account-stream events in seq order at the risk clock `at`; a MandateVersionApplied
# step applies `patch` to the case's base. Every mandate a case applies passes every V-rule.
TW_DAY = {"id": "day_loss", "metric": "realized_loss_usd", "threshold": "100", "action": "end_delegations"}
TW_STREAK = {"id": "losing_streak", "metric": "consecutive_losing_exits", "threshold": "2", "action": "exits_only"}
TW_NEW = {"id": "new_names", "metric": "new_instruments", "threshold": "2", "action": "end_delegations"}
derived("research_equity_tripwired", "research_equity", [{"op": "add", "path": "/autonomy/tripwires", "value": [TW_DAY, TW_STREAK, TW_NEW]}],
        "adds three tripwires: a realized loss in the risk day, a losing streak, and new instruments")
TWB = "research_equity_tripwired"
TW_DELEGATION = {"id": "d1", "lifts": "rule:large_orders", "when": {"field": "purpose", "op": "in", "value": ["increase", "open"]},
                 "max_order_usd": "1000", "max_orders": 3, "max_total_usd": "3000",
                 "starts_at": "2026-09-20T00:00:00.000000000Z", "expires_at": "2026-10-10T00:00:00.000000000Z",
                 "source_approval_id": None}

def tws(*items):
    return [{"op": "add", "path": "/autonomy/tripwires", "value": list(items)}]

NO_TW = [{"op": "remove", "path": "/autonomy/tripwires"}]

def tw_with(t, **kw):
    return dict(t, **kw)

SEM_W = [
    ("MC-W06", "Three tripwires sorted by id with thresholds in range passes", TWB, [], {}),
    ("MC-W07", "Tripwires out of id order are refused", TWB, tws(TW_STREAK, TW_DAY), {}),
    ("MC-W08", "Two tripwires with one id are refused", TWB, tws(TW_STREAK, tw_with(TW_STREAK, threshold="3")), {}),
    ("MC-W09", "A counted tripwire with a fractional threshold is refused", TWB, tws(tw_with(TW_STREAK, threshold="2.5")), {}),
    ("MC-W10", "A counted tripwire above 1,000 is refused", TWB, tws(tw_with(TW_NEW, threshold="1001")), {}),
    ("MC-W11", "A counted tripwire at exactly 1,000 passes", TWB, tws(tw_with(TW_NEW, threshold="1000")), {}),
    ("MC-W12", "A realized-loss tripwire in fractions of a cent is refused", TWB, tws(tw_with(TW_DAY, threshold="0.001")), {}),
    ("MC-W13", "A realized-loss tripwire above the allocation is refused", TWB, tws(tw_with(TW_DAY, threshold="10000.01")), {}),
    ("MC-W14", "A realized-loss tripwire exactly equal to the allocation passes", TWB, tws(tw_with(TW_DAY, threshold="10000")), {}),
    ("MC-W15", "A tripwire the platform proposed and the owner confirmed passes", TWB, [],
     {"provenance": {"/autonomy/tripwires": PU("platform_proposed")}}),
    ("MC-W16", "A tripwire the platform proposed and nobody confirmed is refused", TWB, [],
     {"provenance": {"/autonomy/tripwires": {"source": "platform_proposed", "confirmed": False}}}),
]
for cid, title, base, patch, ctx in SEM_W:
    m = apply_patch(MB[base], patch)
    assert V.is_valid(m), (cid, [e.message for e in V.iter_errors(m)])
    errs, warns = semantic(m, dict(CTX, **ctx))
    cases.append({"id": cid, "kind": "semantic", "title": title, "base": base, "patch": patch, "context": ctx,
                  "expect": {"violations": errs, "warnings": warns, "worst_case": worst_case(m, dict(CTX, **ctx))}})
with_tw_delegation = apply_patch(MB[TWB], [{"op": "add", "path": "/autonomy/delegations", "value": [TW_DELEGATION]}])
for cid, title, patch in [
    ("MC-W17", "Removing a tripwire is risk-increasing, so a delegation carried with it is refused (V-042)",
     [{"op": "add", "path": "/autonomy/delegations", "value": [TW_DELEGATION]}] + tws(TW_DAY, TW_STREAK)),
    ("MC-W18", "Adding a tripwire is risk-reducing, so carrying the delegation with it passes",
     [{"op": "add", "path": "/autonomy/delegations", "value": [TW_DELEGATION]}]
     + tws(TW_DAY, TW_STREAK, TW_NEW, {"id": "streak_long", "metric": "consecutive_losing_exits", "threshold": "4", "action": "end_delegations"})),
]:
    m = apply_patch(MB[TWB], patch)
    assert V.is_valid(m), cid
    ctx = {"previous_version": with_tw_delegation}
    errs, warns = semantic(m, dict(CTX, **ctx))
    cases.append({"id": cid, "kind": "semantic", "title": title, "base": TWB, "patch": patch, "context": ctx,
                  "expect": {"violations": errs, "warnings": warns, "worst_case": worst_case(m, dict(CTX, **ctx))}})
SCH_W = [
    ("MC-W01", "A tripwire with an unknown metric", tws(tw_with(TW_DAY, metric="unrealized_loss_usd"))),
    ("MC-W02", "A tripwire whose action is paused (rule 13)", tws(tw_with(TW_STREAK, action="paused"))),
    ("MC-W03", "A tripwire without a threshold", tws({k: v for k, v in TW_STREAK.items() if k != "threshold"})),
    ("MC-W04", "A tripwire threshold of 0", tws(tw_with(TW_STREAK, threshold="0"))),
    ("MC-W05", "Twenty-one tripwires", tws(*[dict(TW_STREAK, id=f"t{i:02d}") for i in range(21)])),
]
for cid, title, patch in SCH_W:
    m = apply_patch(MB[TWB], patch)
    cases.append({"id": cid, "kind": "schema", "title": title, "base": TWB, "patch": patch, "expect": {"schema_valid": V.is_valid(m)}})
CH_W = [
    ("MC-W19", "Add a tripwire", "research_equity", tws(TW_STREAK)),
    ("MC-W20", "Lower a tripwire's threshold", TWB, tws(TW_DAY, tw_with(TW_STREAK, threshold="1"), TW_NEW)),
    ("MC-W21", "Make a tripwire's action stricter (end_delegations to exits_only)", TWB, tws(tw_with(TW_DAY, action="exits_only"), TW_STREAK, TW_NEW)),
    ("MC-W22", "Remove a tripwire", TWB, tws(TW_DAY, TW_NEW)),
    ("MC-W23", "Raise a tripwire's threshold", TWB, tws(TW_DAY, tw_with(TW_STREAK, threshold="3"), TW_NEW)),
    ("MC-W24", "Soften a tripwire's action (exits_only to end_delegations)", TWB, tws(TW_DAY, tw_with(TW_STREAK, action="end_delegations"), TW_NEW)),
    ("MC-W25", "Change a tripwire's metric under the same id", TWB, tws(TW_DAY, tw_with(TW_STREAK, metric="new_instruments"), TW_NEW)),
    ("MC-W26", "Lower one tripwire's threshold and remove another", TWB, tws(tw_with(TW_DAY, threshold="50"), TW_STREAK)),
]
for cid, title, base, patch in CH_W:
    old = MB[base]
    new = apply_patch(old, patch)
    assert V.is_valid(new) and semantic(new, CTX)[0] == [], cid
    got, paths = classify(old, new)
    cases.append({"id": cid, "kind": "change", "title": title, "base": base, "patch": patch,
                  "expect": {"classification": got, "changed_paths": paths, "old_version": version(old), "new_version": version(new),
                             "step_up_required": got == "risk_increasing"}})

TW_T0 = T("2026-09-22T14:00:00.000000000Z")

def tw_at(s):
    return fmt(TW_T0 + timedelta(seconds=s))

def deploy(at, patch):
    return {"event": "MandateVersionApplied", "at": tw_at(at), "patch": list(patch)}

def fill(at, side, qty, price, fees="0", inst=ABC, event="FillApplied"):
    return {"event": event, "at": tw_at(at), "instrument": inst, "side": side, "qty": qty, "price": price, "fees": fees}

def day(at):
    return {"event": "RiskDayStarted", "at": tw_at(at)}

def ack(at, tid, assertion, age=60, method="cli_confirm"):
    return {"event": "OwnerAcknowledged", "at": tw_at(at), "tripwire": tid,
            "step_up": {"assertion": assertion, "authenticated_at": tw_at(at - age), "method": method}}

NEXT_DAY = 14 * 3600   # 2026-09-23T04:00:00Z, 00:00 New York (EDT)
BIG = dict(OPEN, order_usd="950", combined_score="0.9", instrument=ABC, asset_class="us_equity", session="regular")
SMALL = dict(OPEN, instrument=ABC, asset_class="us_equity", session="regular")

def tw_case(cid, title, base, steps, probes=()):
    """`probes` are decisions taken after the last step under the version then in effect, at its risk clock."""
    m, run_steps = None, []
    for s in steps:
        if s["event"] == "MandateVersionApplied":
            m = apply_patch(MB[base], s["patch"])
            assert V.is_valid(m) and semantic(m, CTX)[0] == [], (cid, semantic(m, CTX))
            run_steps.append(dict(s, mandate=m))
        else:
            run_steps.append(s)
    out = tripwire_run(run_steps)
    st = tripwire_autonomy_state(out[-1]["state"], steps[-1]["at"])
    pr = []
    for a in probes:
        e = autonomy(m, a, st)
        pr.append({"action": a, "expect": {k: v for k, v in e.items() if k in ("decision", "by", "delegation_id", "lifted")}})
    case = {"id": cid, "kind": "tripwire", "title": title, "base": base, "context": {"environment": "paper"}, "steps": steps,
            "expect": out}
    if pr:
        case["probes"] = pr
    cases.append(case)

STREAK_ONLY = tws(TW_STREAK)
DAY_ONLY = tws(TW_DAY)
NEW_ONLY = tws(TW_NEW)
DELEG = [{"op": "add", "path": "/autonomy/delegations", "value": [TW_DELEGATION]}]
EXITS = [{"purpose": "discretionary_exit"}, {"purpose": "owner_exit"}, {"purpose": "risk_exit"}]
tw_case("MC-W27", "Two losing exits in a row fire an exits_only tripwire at the second, not the first", TWB,
        [deploy(0, STREAK_ONLY + DELEG), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"), fill(30, "sell", "5", "98")],
        [BIG] + EXITS)
tw_case("MC-W28", "A winning exit between two losing ones resets the streak, so nothing fires", TWB,
        [deploy(0, STREAK_ONLY), fill(10, "buy", "9", "100"), fill(20, "sell", "3", "99"), fill(30, "sell", "3", "101"),
         fill(40, "sell", "3", "99")])
tw_case("MC-W29", "A buy between two losing exits does not break the streak, and an exit at cost loses its fee", TWB,
        [deploy(0, STREAK_ONLY), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "100", fees="0.01"),
         fill(30, "buy", "5", "100"), fill(40, "sell", "10", "100", fees="0.01")])
tw_case("MC-W30", "A realized loss in the risk day reaching its threshold ends the delegations and leaves the mode normal", TWB,
        [deploy(0, DAY_ONLY + DELEG), fill(10, "buy", "20", "100"), fill(20, "sell", "10", "95"), fill(30, "sell", "10", "95")],
        [BIG, SMALL] + EXITS)
tw_case("MC-W31", "A realized loss one cent short of the threshold fires nothing, and the delegation still lifts", TWB,
        [deploy(0, DAY_ONLY + DELEG), fill(10, "buy", "20", "100"), fill(20, "sell", "19", "95"), fill(30, "sell", "1", "95.01")],
        [BIG])
tw_case("MC-W32", "The realized loss counts afresh from 00:00 New York, so two days' losses do not add", TWB,
        [deploy(0, DAY_ONLY), fill(10, "buy", "20", "100"), fill(20, "sell", "10", "94"), day(NEXT_DAY),
         fill(NEXT_DAY + 3600, "sell", "10", "94")])
tw_case("MC-W33", "A buy's fees are realized loss: commissions alone reach the threshold", TWB,
        [deploy(0, DAY_ONLY), fill(10, "buy", "1", "100", fees="60"), fill(20, "buy", "1", "100", fees="40")])
tw_case("MC-W34", "The first fill in a second never-held instrument fires new_instruments; re-entering one held before does not count", TWB,
        [deploy(0, NEW_ONLY), fill(10, "buy", "1", "100", inst=ABC), fill(20, "sell", "1", "100", inst=ABC),
         fill(30, "buy", "1", "100", inst=ABC), fill(40, "buy", "1", "50", inst=XYZ)])
tw_case("MC-W35", "Fills before a tripwire is armed do not count toward it", TWB,
        [deploy(0, NO_TW), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"), fill(30, "sell", "4", "98"),
         deploy(40, STREAK_ONLY), fill(50, "sell", "1", "97")])
tw_case("MC-W36", "Lowering a threshold to the count already reached fires at the version's own input", TWB,
        [deploy(0, tws(tw_with(TW_STREAK, threshold="3"))), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"),
         fill(30, "sell", "4", "98"), deploy(40, STREAK_ONLY)])
tw_case("MC-W37", "Removing a fired tripwire in a version does not lift it; the owner's acknowledgment does", TWB,
        [deploy(0, STREAK_ONLY), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"), fill(30, "sell", "5", "98"),
         deploy(40, NO_TW), ack(50, "losing_streak", "as-37")])
tw_case("MC-W38", "An acknowledgment with step-up 301 s old is refused and the tripwire stays fired", TWB,
        [deploy(0, STREAK_ONLY), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"), fill(30, "sell", "5", "98"),
         ack(40, "losing_streak", "as-38", age=301)])
tw_case("MC-W39", "A reused step-up assertion is refused; a fresh one lifts and the tripwire counts afresh", TWB,
        [deploy(0, STREAK_ONLY), ack(5, "losing_streak", "as-39"), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"),
         fill(30, "sell", "4", "98"), ack(40, "losing_streak", "as-39"), ack(50, "losing_streak", "as-39b"),
         fill(60, "sell", "1", "97")])
tw_case("MC-W40", "After an acknowledgment, two new losing exits fire it again", TWB,
        [deploy(0, STREAK_ONLY), fill(10, "buy", "10", "100"), fill(20, "sell", "4", "99"), fill(30, "sell", "4", "98"),
         ack(40, "losing_streak", "as-40"), fill(50, "sell", "1", "97"), fill(60, "sell", "1", "96")])
tw_case("MC-W41", "Tightening a fired end_delegations tripwire to exits_only holds new openings at once", TWB,
        [deploy(0, DAY_ONLY), fill(10, "buy", "20", "100"), fill(20, "sell", "20", "95"),
         deploy(30, tws(tw_with(TW_DAY, action="exits_only")))])
tw_case("MC-W42", "Softening a fired exits_only tripwire to end_delegations keeps it holding new openings until acknowledged", TWB,
        [deploy(0, STREAK_ONLY), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"), fill(30, "sell", "5", "98"),
         deploy(40, tws(tw_with(TW_STREAK, action="end_delegations"))), ack(50, "losing_streak", "as-42")])
tw_case("MC-W43", "Two tripwires fire on one fill, in id order, each alerting with opaque text only", TWB,
        [deploy(0, tws(tw_with(TW_DAY, threshold="10"), tw_with(TW_STREAK, threshold="1"))), fill(10, "buy", "10", "100"),
         fill(20, "sell", "10", "98")])
tw_case("MC-W44", "Changing a tripwire's metric under the same id arms it afresh", TWB,
        [deploy(0, tws(tw_with(TW_STREAK, threshold="3"))), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"),
         fill(30, "sell", "4", "98"), deploy(40, tws(tw_with(TW_STREAK, metric="new_instruments", threshold="1"))),
         fill(50, "sell", "1", "97"), fill(60, "buy", "1", "50", inst=XYZ)])
tw_case("MC-W45", "Acknowledging a tripwire that has not fired changes nothing", TWB,
        [deploy(0, STREAK_ONLY), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"), ack(30, "losing_streak", "as-45"),
         fill(40, "sell", "5", "98")])
tw_case("MC-W46", "A late fill counts when it is applied, like any fill", TWB,
        [deploy(0, STREAK_ONLY), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"),
         fill(30, "sell", "5", "98", event="LateFillApplied")])
tw_case("MC-W47", "A partial exit removes basis rounded half-even at 12 places: a tie rounds to even, so the exit breaks even and is not a loss", TWB,
        [deploy(0, tws(tw_with(TW_STREAK, threshold="1"))), fill(10, "buy", "1", "10"), fill(20, "buy", "1", "10.000000000001"),
         fill(30, "sell", "1", "10")])

tw_case("MC-W48", "A losing streak survives 00:00 New York: one losing exit either side of midnight fires it", TWB,
        [deploy(0, STREAK_ONLY), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"), day(NEXT_DAY),
         fill(NEXT_DAY + 3600, "sell", "5", "98")])
tw_case("MC-W49", "A refused acknowledgment still spends its assertion, so replaying it is refused as reused", TWB,
        [deploy(0, STREAK_ONLY), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"), fill(30, "sell", "5", "98"),
         ack(40, "losing_streak", "as-49", method="password"), ack(50, "losing_streak", "as-49")])

def ack_by(at, tid, assertion, user, requester):
    return dict(ack(at, tid, assertion), user=user, requester=requester, independent_approval_required=True)

tw_case("MC-W50", "Under independent approval the user who requested the lift cannot acknowledge it", TWB,
        [deploy(0, STREAK_ONLY), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"), fill(30, "sell", "5", "98"),
         ack_by(40, "losing_streak", "as-50", "user:u1", "user:u1")])
tw_case("MC-W51", "Under independent approval a second user's acknowledgment lifts it", TWB,
        [deploy(0, STREAK_ONLY), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"), fill(30, "sell", "5", "98"),
         ack_by(40, "losing_streak", "as-51", "user:u2", "user:u1")])
tw_case("MC-W53", "Under independent approval an acknowledgment that names no requester is refused (fail closed)", TWB,
        [deploy(0, STREAK_ONLY), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"), fill(30, "sell", "5", "98"),
         ack_by(40, "losing_streak", "as-53", "user:u2", None)])
tw_case("MC-W54", "Under independent approval an acknowledgment that names no acknowledging user is refused (fail closed)", TWB,
        [deploy(0, STREAK_ONLY), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"), fill(30, "sell", "5", "98"),
         ack_by(40, "losing_streak", "as-54", None, "user:u1")])
tw_case("MC-W55", "Independent approval on when the lift was requested binds it though the policy is off at processing", TWB,
        [deploy(0, STREAK_ONLY), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"), fill(30, "sell", "5", "98"),
         dict(ack_by(40, "losing_streak", "as-55", "user:u1", "user:u1"), independent_now=False)])
tw_case("MC-W56", "Independent approval turned on after the lift was requested binds it at processing", TWB,
        [deploy(0, STREAK_ONLY), fill(10, "buy", "10", "100"), fill(20, "sell", "5", "99"), fill(30, "sell", "5", "98"),
         dict(ack_by(40, "losing_streak", "as-56", "user:u1", "user:u1"), independent_approval_required=False, independent_now=True)])
TW_RISK = WIDE + FAST + [rep("/risk/max_daily_loss", "0.5"), {"op": "add", "path": "/autonomy/tripwires", "value": [TW_STREAK]}]
risk_case("MC-W52", "A fired tripwire is a latched limit: an allocation increase is rejected until the owner acknowledges it",
          "two_stock_swing", TW_RISK, "10", "100", "us_equity", at(14, 0),
          [{"event": "fill", "at": at(14, 0, 10), "side": "sell", "qty": "5", "price": "99", "session": "regular"},
           {"event": "fill", "at": at(14, 0, 20), "side": "sell", "qty": "4", "price": "98", "session": "regular"},
           {"event": "allocation_change", "at": at(14, 0, 30), "delta_usd": "1000", "session": "regular"},
           {"event": "owner_acknowledged", "at": at(14, 0, 40), "restriction": "tripwire:losing_streak", "session": "regular",
            "step_up": {"assertion": "as-52", "authenticated_at": at(14, 0, 30), "method": "cli_confirm"}},
           {"event": "allocation_change", "at": at(14, 0, 50), "delta_usd": "1000", "session": "regular"}],
          note="The tripwire fold reads the risk state's one instrument; fees are 0, as in every risk-state case.")
TW_RISK_DAY = WIDE + FAST + [rep("/risk/max_daily_loss", "0.5"), {"op": "add", "path": "/autonomy/tripwires", "value": [TW_DAY]}]
risk_case("MC-W57", "An end_delegations tripwire latches too, and the realized loss counts from 00:00 New York, so a profitable day does not offset the next day's loss",
          "two_stock_swing", TW_RISK_DAY, "10", "100", "us_equity", at(14, 0),
          [{"event": "fill", "at": at(14, 0, 10), "side": "sell", "qty": "5", "price": "200", "session": "regular"},
           {"event": "risk_day_started", "at": at(4, 0, 0, day=22), "session": "pre_market"},
           {"event": "fill", "at": at(14, 0, 10, day=22), "side": "sell", "qty": "5", "price": "60", "session": "regular"},
           {"event": "allocation_change", "at": at(14, 0, 20, day=22), "delta_usd": "1000", "session": "regular"}],
          note="Day one realizes +500 and day two -200: with the risk-day reset the tripwire fires at 200; without it the day's net would be a gain.")

W0 = next(i for i, c in enumerate(cases) if c["id"].startswith("MC-W"))
assert all(c["id"].startswith("MC-W") for c in cases[W0:])
cases[W0:] = sorted(cases[W0:], key=lambda c: c["id"])

# =========================================================== output
HEADER = """# Reference cases for docs/specs/mandate.md (spec v0.7)
#
# Generated by a reference implementation that is fuzzed against invariants MI-1 to MI-32
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
#   `stop_limit_asset_classes` is the asset classes the connection's profile protects with a
#   stop-limit (spec 4, DEC-539); null means no profile was supplied, and then every allowed asset
#   class counts as protected by one (V-008 and W-002 fail closed).
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
# - review: an autonomy case at the risk clock `now`, which judges the review date (spec 6.2 step 5b)
#   and the delegations' windows (spec 6.5); no delegation has been used and nothing suspends one.
#   An `ask` also expects the approval content's `trigger` (spec 6.4) for a request the agent made.
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
# - tripwire: fold spec 6.7 over `steps` (account-stream inputs at the risk clock `at`, in seq order). A
#   MandateVersionApplied step applies `patch` to `base`; a fill carries its instrument, side, qty, price, and
#   its own fees; OwnerAcknowledged names the tripwire and carries step-up evidence judged at `at` in
#   `context.environment` (spec 6.1). `expect[i]` is input i's journal, in order, and the state after it:
#   `fired` (id -> the action it holds), `restriction` (`exits_only` or null), `delegations_suspended`, and
#   each armed tripwire's metric. `probes` are spec 6.2 decisions after the last step under the version
#   then in effect, at that step's risk clock, with that state (no delegation used yet).
# - escalation: the approval lifecycle of spec 6.1 and 6.4. `op: lifecycle` runs `script` from `start` under
#   `context` (approvers, author, environment, timeout_s, cli_inbox, push channels, quiet hours): `ask` binds
#   `bound` as approval `ap1`; `tick` is the scheduler's ClockAdvanced, then expiry; `fold` folds `event` and steps
#   nothing (its clock still advances the folded clock); `response` is one ApprovalResponseSubmitted re-validated
#   against `now`; `cancel` and `batch` cancel every pending approval with `reason`, and `batch` then judges its
#   responses in the same step. `expect[i].drafts` are the agent-stream drafts of input i, in order. `op:
#   ask_permit` judges each query against `ledger` (spec 6.4, asking is bounded); `op: deliver_now` judges each
#   query's channel under `quiet_hours`.
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
