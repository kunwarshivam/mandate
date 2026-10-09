"""Base mandates and the signal-model registry, shared by the generator and the fuzz."""
import copy
from ref import V, semantic

BTC = "7b4a1c2e-1111-4a2b-9c3d-000000000001"
XYZ = "7b4a1c2e-2222-4a2b-9c3d-000000000002"
QRS = "7b4a1c2e-3333-4a2b-9c3d-000000000003"
LMN = "7b4a1c2e-4444-4a2b-9c3d-000000000004"
ABC = "7b4a1c2e-5555-4a2b-9c3d-000000000005"
H_MR = "sha256:" + "1" * 64
H_MOM = "sha256:" + "2" * 64
H_NEWS = "sha256:" + "3" * 64
H_RES = "sha256:" + "4" * 64

LADDER = [
    {"at": "0.03", "action": "scale_sizes", "factor": "0.5"},
    {"at": "0.06", "action": "exits_only", "factor": None},
    {"at": "0.08", "action": "flatten_and_pause", "factor": None},
]
RULES = [
    {"id": "large_orders", "when": {"field": "order_usd", "op": "gt", "value": "900"}, "then": "ask"},
    {"id": "low_score", "when": {"field": "combined_score", "op": "lt", "value": "0.65"}, "then": "ask"},
    {"id": "routine", "when": {"field": "purpose", "op": "in", "value": ["increase", "open"]}, "then": "auto"},
]
RISK = {"max_position_usd": "10000", "max_position_fraction": "1", "max_gross_exposure_usd": "10000",
        "max_order_usd": "1000", "max_orders_per_day": 50, "max_daily_loss": "0.02", "daily_loss_action": "exits_only",
        "max_drawdown": "0.08", "drawdown_ladder": LADDER, "hysteresis": "0.01", "scale_action": "limit_buys",
        "breach_confirm_s": 60, "daily_breach_min_s": 3600, "scale_lift_after_s": 600, "reentry_cooldown_s": 3600}

btc = {
    "mandate_schema_version": 1, "name": "btc-accumulator", "source_text_ref": "sha256:" + "a" * 64,
    "environment": "paper", "connection_id": "conn_alpaca_paper_01",
    "capital": {"allocation_usd": "10000", "max_loss_from_allocation": "0.1"},
    "goal": {"type": "accumulate", "instrument": BTC, "target_qty": "0.15", "max_avg_price": "58000",
             "max_spend_usd": "9000", "end_date": "2026-12-31", "on_complete": "hold_protected"},
    "universe": {"pinned": True, "pinned_instruments": [{"asset_id": BTC, "symbol": "BTC/USD", "asset_class": "crypto"}],
                 "max_instruments": 1, "asset_classes": ["crypto"],
                 "leveraged_etps_enabled": False, "leveraged_etp_disclosure_version": None},
    "behavior": {
        "description": "Buy dips; avoid trading 30 minutes around major macro releases.",
        "signal_models": [{"id": "quant.mean_reversion", "version": "1.0.0", "content_hash": H_MR,
                           "params": [{"key": "lookback_bars", "value": "20"}, {"key": "z_entry", "value": "1.5"}],
                           "weight": "1", "max_output_age_s": 1800, "admits_instruments": False}],
        "research": None,
        "cadence": {"interval_s": 900, "event_sources": ["news", "price"]},
        "sizing": {"method": "conviction_linear", "entry_threshold": "0.3", "exit_threshold": "0.3",
                   "rebalance_band": "0.05"}},
    "protection": {"enabled": True, "stop_distance": "0.08", "take_profit_distance": None,
                   "crypto_stop_limit_offset": "0.005"},
    "risk": copy.deepcopy(RISK),
    "autonomy": {"rules": copy.deepcopy(RULES), "default": "ask", "admission": "ask",
                 "approval": {"timeout_s": 600, "on_timeout": "skip", "approvers": ["role:approver"],
                              "two_approver_above_usd": None}},
    "notifications": {"channels": ["email", "web_push"],
                      "quiet_hours": {"start": "23:00", "end": "07:00", "timezone": "America/New_York"}},
}

swing = copy.deepcopy(btc)
swing.update({
    "name": "two-stock-swing", "source_text_ref": None,
    "goal": {"type": "profit_stop", "profit_level": "0.1", "end_date": None},
    "universe": {"pinned": True,
                 "pinned_instruments": [{"asset_id": XYZ, "symbol": "XYZ", "asset_class": "us_equity"},
                                        {"asset_id": QRS, "symbol": "QRS", "asset_class": "us_equity"}],
                 "max_instruments": 2, "asset_classes": ["us_equity"],
                 "leveraged_etps_enabled": False, "leveraged_etp_disclosure_version": None},
    "protection": {"enabled": True, "stop_distance": "0.05", "take_profit_distance": "0.1",
                   "crypto_stop_limit_offset": None},
})
swing["behavior"] = {
    "description": "Swing trade two stocks on momentum and news.",
    "signal_models": [
        {"id": "llm.news_research", "version": "0.3.0", "content_hash": H_NEWS, "params": [], "weight": "0.4",
         "max_output_age_s": 3600, "admits_instruments": False},
        {"id": "quant.momentum", "version": "1.0.0", "content_hash": H_MOM,
         "params": [{"key": "lookback_bars", "value": "50"}], "weight": "0.6", "max_output_age_s": 900,
         "admits_instruments": False},
    ],
    "research": None,
    "cadence": {"interval_s": 300, "event_sources": ["fills", "news", "price"]},
    "sizing": {"method": "conviction_linear", "entry_threshold": "0.3", "exit_threshold": "0.3", "rebalance_band": "0.05"}}
swing["risk"].update({"max_position_usd": "1500", "max_position_fraction": "0.2", "max_gross_exposure_usd": "2000"})

research = copy.deepcopy(swing)
research.update({
    "name": "research-equity", "source_text_ref": None,
    "goal": {"type": "continuous", "end_date": None, "on_complete": "hold_protected"},
    "universe": {"pinned": False, "pinned_instruments": [], "max_instruments": 5, "asset_classes": ["us_equity"],
                 "leveraged_etps_enabled": False, "leveraged_etp_disclosure_version": None},
})
research["behavior"] = {
    "description": "Hold a small book of US equities on the research agent's theses.",
    "signal_models": [
        {"id": "llm.research_agent", "version": "0.1.0", "content_hash": H_RES, "params": [], "weight": "0.5",
         "max_output_age_s": 3600, "admits_instruments": True},
        {"id": "quant.momentum", "version": "1.0.0", "content_hash": H_MOM,
         "params": [{"key": "lookback_bars", "value": "50"}], "weight": "0.5", "max_output_age_s": 900,
         "admits_instruments": False},
    ],
    "research": {"interval_s": 3600, "cost_cap_usd_per_day": "5", "max_revisions_per_lineage": 3},
    "cadence": {"interval_s": 300, "event_sources": ["fills", "news", "price"]},
    "sizing": {"method": "conviction_linear", "entry_threshold": "0.3", "exit_threshold": "0.3",
               "rebalance_band": "0.05"}}

BASES = {"btc_accumulator": btc, "two_stock_swing": swing, "research_equity": research}
REGISTRY = {"quant.mean_reversion": {"version": "1.0.0", "content_hash": H_MR, "params": ["lookback_bars", "z_entry"]},
            "quant.momentum": {"version": "1.0.0", "content_hash": H_MOM, "params": ["lookback_bars"]},
            "llm.news_research": {"version": "0.3.0", "content_hash": H_NEWS, "params": []},
            "llm.research_agent": {"version": "0.1.0", "content_hash": H_RES, "params": [], "admits_instruments": True}}
CTX = {"account_equity_usd": "25000", "other_allocations_usd": "0", "validation_date": "2026-09-24",
       "registry": REGISTRY, "approver_users": 1, "workspace_users": 1,
       "stop_limit_asset_classes": ["crypto"]}
"""The bases' connection is Alpaca paper, whose profile protects only crypto with a stop-limit (trading spec §5.2), so
the defaults state it: absent, validation fails closed and every allowed asset class needs the offset (DEC-539)."""

for _n, _m in BASES.items():
    V.validate(_m)
    assert semantic(_m, CTX)[0] == [], (_n, semantic(_m, CTX))
