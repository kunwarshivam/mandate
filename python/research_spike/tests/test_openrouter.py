import json
from decimal import Decimal

import pytest

from research_spike import openrouter

from .conftest import FakeTransport, fixture_text


def test_prices_are_fetched_once_and_cached(tmp_path):
    transport = FakeTransport([("api/v1/models", fixture_text("models.json"))])
    prices = openrouter.model_prices("anthropic/claude-sonnet-5", tmp_path, transport)
    assert prices == openrouter.Prices(Decimal("0.000003"), Decimal("0.000015"))
    assert openrouter.model_prices("openai/gpt-5", tmp_path, transport).prompt_per_token == Decimal(
        "0.00000125"
    )
    assert len(transport.requests) == 1 and (tmp_path / "openrouter-models.json").exists()
    with pytest.raises(openrouter.OpenRouterError, match="not in the OpenRouter catalogue"):
        openrouter.model_prices("nobody/nothing", tmp_path, transport)


def test_cost_is_computed_from_per_token_prices():
    prices = openrouter.Prices(Decimal("0.000003"), Decimal("0.000015"))
    assert openrouter.cost(prices, 4000, 200) == Decimal("0.015000")


def test_completion_captures_usage_cost_and_json_request(tmp_path):
    transport = FakeTransport([("chat/completions", fixture_text("completion.json"))])
    prices = openrouter.Prices(Decimal("0.000003"), Decimal("0.000015"))
    completion = openrouter.complete(
        "anthropic/claude-sonnet-5", "sys", "user", prices, "sk-or-test", transport
    )
    assert (completion.prompt_tokens, completion.completion_tokens, completion.cost_usd) == (
        4000,
        200,
        Decimal("0.015000"),
    )
    assert (
        json.loads(completion.content)["theses"][0]["instrument"] == "AAPL"
        and completion.request_id == "gen-0001"
    )
    method, url, headers, body = transport.requests[0]
    sent = json.loads(body)
    assert method == "POST" and url == openrouter.CHAT_URL and headers["Authorization"] == "Bearer sk-or-test"
    assert sent["response_format"] == {"type": "json_object"} and sent["messages"][0]["content"] == "sys"


def test_malformed_completions_raise():
    with pytest.raises(openrouter.OpenRouterError, match="malformed"):
        openrouter.complete(
            "m", "s", "u", openrouter.Prices(Decimal(0), Decimal(0)), "k", FakeTransport([("chat", "{}")])
        )
