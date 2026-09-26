"""Chat completions through OpenRouter with JSON output, token usage, and cost from the model's prices."""

import json
import os
from dataclasses import dataclass
from decimal import Decimal
from pathlib import Path

from research_spike.http import Transport, request_json, urllib_transport

CHAT_URL = "https://openrouter.ai/api/v1/chat/completions"
MODELS_URL = "https://openrouter.ai/api/v1/models"
API_KEY_ENV = "MANDATE_OPENROUTER_API_KEY"
PRICES_FILE = "openrouter-models.json"


class OpenRouterError(Exception):
    pass


@dataclass(frozen=True)
class Prices:
    prompt_per_token: Decimal
    completion_per_token: Decimal


@dataclass(frozen=True)
class Completion:
    model: str
    content: str
    prompt_tokens: int
    completion_tokens: int
    cost_usd: Decimal
    request_id: str


def api_key() -> str:
    key = os.environ.get(API_KEY_ENV)
    if not key:
        raise OpenRouterError(f"set {API_KEY_ENV}")
    return key


def model_prices(model: str, data_dir: Path, transport: Transport = urllib_transport) -> Prices:
    cache = data_dir / PRICES_FILE
    if cache.exists():
        catalogue = json.loads(cache.read_text(encoding="utf-8"), parse_float=Decimal)
    else:
        catalogue = request_json(transport, "GET", MODELS_URL, {"Accept": "application/json"})
        cache.write_text(json.dumps(catalogue, default=str), encoding="utf-8")
    for entry in catalogue.get("data", []):
        if entry.get("id") == model:
            pricing = entry["pricing"]
            return Prices(Decimal(str(pricing["prompt"])), Decimal(str(pricing["completion"])))
    raise OpenRouterError(f"model {model} is not in the OpenRouter catalogue")


def cost(prices: Prices, prompt_tokens: int, completion_tokens: int) -> Decimal:
    total = prices.prompt_per_token * prompt_tokens + prices.completion_per_token * completion_tokens
    return total.quantize(Decimal("0.000001"))


def complete(
    model: str,
    system: str,
    user: str,
    prices: Prices,
    key: str,
    transport: Transport = urllib_transport,
) -> Completion:
    body = {
        "model": model,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        "response_format": {"type": "json_object"},
        "temperature": 0,
    }
    headers = {"Authorization": f"Bearer {key}", "Accept": "application/json"}
    reply = request_json(transport, "POST", CHAT_URL, headers, body=body)
    try:
        content = reply["choices"][0]["message"]["content"]
        usage = reply.get("usage") or {}
        prompt_tokens = int(usage.get("prompt_tokens", 0))
        completion_tokens = int(usage.get("completion_tokens", 0))
    except (KeyError, IndexError, TypeError) as error:
        raise OpenRouterError(f"malformed completion: {error}") from error
    return Completion(
        model=str(reply.get("model", model)),
        content=content,
        prompt_tokens=prompt_tokens,
        completion_tokens=completion_tokens,
        cost_usd=cost(prices, prompt_tokens, completion_tokens),
        request_id=str(reply.get("id", "")),
    )
