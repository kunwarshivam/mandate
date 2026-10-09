"""The tracer's mandate fixtures, recomputed from the rules rather than typed (AGENTS.md).

Run from the repository root with the reference environment:

    uv run --no-project --python 3.14 --with-requirements reference/mandate/requirements.txt \
        python crates/mandate-shell/tests/fixtures/tracer/generate.py

Each fixture passes the schema and every semantic rule of reference/mandate/ref.py, and the order
builder's answer for it at the fixture quote is asserted here: one share, AUTO, for `mandate.json`;
ASK for `mandate-ask.json`; DENY by its first rule for `mandate-deny.json`, whose later `auto` rule
still exceeds a policy that forbids `auto`; a hold for `mandate-small-orders.json`, whose order cap is below one
share. AAPL is internal test data (DEC-90), not an instrument recommendation.
"""

import copy
import hashlib
import json
import sys
from decimal import Decimal
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
sys.path.insert(0, str(ROOT / "reference" / "mandate"))

from bases import CTX, REGISTRY, RISK  # noqa: E402
from ref import V, builder, semantic  # noqa: E402

AAPL = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415"
MODEL_ARTIFACT = b'{"id":"quant.ma_crossover","version":"1.0.0"}\n'
H_MA = "sha256:" + hashlib.sha256(MODEL_ARTIFACT).hexdigest()
NOW = "2026-09-25T20:00:00.000000000Z"
EXPIRES = "2026-09-26T20:00:00.000000000Z"
BID, ASK = "255.10", "255.20"


def base():
    risk = copy.deepcopy(RISK)
    risk.update(
        {
            "max_position_usd": "1000",
            "max_position_fraction": "1",
            "max_gross_exposure_usd": "1000",
            "max_order_usd": "300",
        }
    )
    return {
        "mandate_schema_version": 1,
        "name": "tracer-aapl",
        "source_text_ref": None,
        "environment": "paper",
        "connection_id": "conn_alpaca_paper_01",
        "capital": {"allocation_usd": "1000", "max_loss_from_allocation": "0.1"},
        "goal": {"type": "continuous", "end_date": None, "on_complete": "hold_protected"},
        "universe": {
            "pinned": True,
            "pinned_instruments": [{"asset_id": AAPL, "symbol": "AAPL", "asset_class": "us_equity"}],
            "max_instruments": 1,
            "asset_classes": ["us_equity"],
            "leveraged_etps_enabled": False,
            "leveraged_etp_disclosure_version": None,
        },
        "behavior": {
            "description": "Tracer bullet: one share of AAPL on the moving-average baseline (internal test data).",
            "signal_models": [
                {
                    "id": "quant.ma_crossover",
                    "version": "1.0.0",
                    "content_hash": H_MA,
                    "params": [
                        {"key": "fast_periods", "value": "5"},
                        {"key": "slow_periods", "value": "20"},
                    ],
                    "weight": "1",
                    "max_output_age_s": 86400,
                    "admits_instruments": False,
                }
            ],
            "research": None,
            "cadence": {"interval_s": 86400, "event_sources": ["price"]},
            "sizing": {
                "method": "conviction_linear",
                "entry_threshold": "0.3",
                "exit_threshold": "0.3",
                "rebalance_band": "0.05",
            },
        },
        "protection": {
            "enabled": True,
            "stop_distance": "0.05",
            "take_profit_distance": "0.1",
            "crypto_stop_limit_offset": None,
        },
        "risk": risk,
        "autonomy": {
            "rules": [
                {
                    "id": "routine",
                    "when": {"field": "purpose", "op": "in", "value": ["increase", "open"]},
                    "then": "auto",
                }
            ],
            "default": "ask",
            "admission": "ask",
            "approval": {
                "timeout_s": 600,
                "on_timeout": "skip",
                "approvers": ["role:approver"],
                "two_approver_above_usd": None,
            },
        },
        "notifications": {
            "channels": ["email"],
            "quiet_hours": {"start": "23:00", "end": "07:00", "timezone": "America/New_York"},
        },
    }


def sized(mandate):
    return builder(
        mandate,
        {
            "now": NOW,
            "instrument": AAPL,
            "asset_class": "us_equity",
            "agent_equity": "1000",
            "position_qty": "0",
            "quote": {"bid": BID, "ask": ASK},
            "qty_increment": "1",
            "min_order_usd": "1",
            "gate_state": {
                "agent_equity": "1000",
                "positions_mv": {},
                "working_opening_orders": [],
                "orders_today": 0,
                "last_exit_fill_at": {},
                "working_universe": [AAPL],
            },
            "outputs": [
                {
                    "model_id": "quant.ma_crossover",
                    "model_version": "1.0.0",
                    "content_hash": H_MA,
                    "instrument_id": AAPL,
                    "as_of": NOW,
                    "expires_at": EXPIRES,
                    "conviction": "1",
                    "confidence": "1",
                }
            ],
        },
    )


def main():
    entry = Decimal(ASK)
    assert entry * (Decimal("1") - Decimal("0.05")) == Decimal("242.4400")
    assert entry * (Decimal("1") + Decimal("0.1")) == Decimal("280.720")
    (HERE / "model-artifact.json").write_bytes(MODEL_ARTIFACT)
    assert hashlib.sha256((HERE / "model-artifact.json").read_bytes()).hexdigest() == H_MA.removeprefix(
        "sha256:"
    )
    registry = dict(REGISTRY)
    registry["quant.ma_crossover"] = {
        "version": "1.0.0",
        "content_hash": H_MA,
        "params": ["fast_periods", "slow_periods"],
    }
    ctx = dict(CTX, registry=registry, account_equity_usd="100000")

    one_share = base()
    ask = base()
    ask["name"] = "tracer-aapl-ask"
    ask["autonomy"]["rules"][0]["then"] = "ask"
    deny = base()
    deny["name"] = "tracer-aapl-deny"
    deny["autonomy"]["rules"].insert(
        0, {"id": "no_opens", "then": "deny", "when": {"field": "purpose", "op": "in", "value": ["open"]}}
    )
    small = base()
    small["name"] = "tracer-aapl-small-orders"
    small["risk"]["max_order_usd"] = "100"

    expected = {
        "mandate.json": (one_share, {"action": "buy", "qty": "1", "limit_price": "255.2", "autonomy": "auto"}),
        "mandate-ask.json": (ask, {"action": "buy", "qty": "1", "limit_price": "255.2", "autonomy": "ask"}),
        "mandate-deny.json": (deny, {"action": "buy", "qty": "1", "limit_price": "255.2", "autonomy": "deny"}),
        "mandate-small-orders.json": (small, {"action": "hold"}),
    }
    for name, (mandate, want) in expected.items():
        V.validate(mandate)
        violations = semantic(mandate, ctx)[0]
        assert violations == [], (name, violations)
        got = sized(mandate)
        for key, value in want.items():
            actual = got["autonomy"]["decision"] if key == "autonomy" else got.get(key)
            assert actual == value, (name, key, actual, got)
        (HERE / name).write_text(json.dumps(mandate, indent=2, sort_keys=True) + "\n")
        print(name, want)


if __name__ == "__main__":
    main()
