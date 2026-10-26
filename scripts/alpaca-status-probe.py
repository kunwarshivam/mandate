"""Ask Alpaca's stock stream whether the paper data plan takes trading-status and LULD subscriptions.

Trading spec §4.4 needs the instrument's trading status and LULD bands. Alpaca's stock stream names
both channels, `statuses` and `lulds`, but its documentation says only that statuses are available
"depending on your subscription", and nothing of the free `iex` feed. This probe asks the stream
once and prints only what the stream answered to the sign-in and the subscription, then listens a
short while and names the message types that arrived: message types, codes, and channel names. It
never prints the key, the secret, a price or a time, and it places no order: it talks only to the
market-data stream host.

A confirmed subscription is not proof that the feed delivers statuses: a halt is rare, and the
listen window may see none. The result says which it saw.

Run it once, from a machine that holds the founder's Alpaca paper key in its environment, with no
other Alpaca data stream open on that key (the free plan allows one connection):

    APCA_API_KEY_ID=... APCA_API_SECRET_KEY=... \
        uv run --no-project --with 'websockets==15.0.1' python scripts/alpaca-status-probe.py

`websockets` (no dependencies of its own) is fetched for this one run only, into uv's cache; the
probe adds no dependency to any workspace project. The key is read from the environment only.

Exit codes: 0 the subscription was confirmed for both channels; 1 it was not; 2 the sign-in was
refused; 3 the key is not in the environment; 4 inconclusive (no connection, a timeout, or an
answer that did not read).
"""

import asyncio
import json
import os
import sys
import time

import websockets

STREAM = "wss://stream.data.alpaca.markets/v2/iex"
SYMBOLS = ["SPY", "QQQ", "AAPL", "TSLA", "NVDA"]
SAFE_KEYS = ("T", "msg", "code", "statuses", "lulds", "trades", "quotes", "bars")
ANSWER_S = 15
LISTEN_S = 20


def safe(item: dict) -> dict:
    """Only the members that describe the answer: never a price, a time, or anything sent."""
    return {key: item[key] for key in SAFE_KEYS if key in item}


def items(frame) -> list[dict]:
    parsed = json.loads(frame)
    listed = parsed if isinstance(parsed, list) else [parsed]
    return [item for item in listed if isinstance(item, dict)]


async def answer(socket, wanted: tuple[str, ...]) -> list[dict]:
    """The frames up to and including the first whose type is one of `wanted`, within the deadline."""
    deadline = time.monotonic() + ANSWER_S
    seen: list[dict] = []
    while True:
        left = deadline - time.monotonic()
        if left <= 0:
            raise TimeoutError
        got = items(await asyncio.wait_for(socket.recv(), timeout=left))
        seen += got
        if any(item.get("T") in wanted for item in got):
            return [safe(item) for item in seen]


async def listen(socket) -> list[str]:
    """The message types that arrive in the listen window, each once."""
    types: list[str] = []
    end = time.monotonic() + LISTEN_S
    while (left := end - time.monotonic()) > 0:
        try:
            got = items(await asyncio.wait_for(socket.recv(), timeout=left))
        except TimeoutError:
            break
        for item in got:
            kind = str(item.get("T"))
            if kind not in types:
                types.append(kind)
    return types


async def probe(key: str, secret: str) -> int:
    async with websockets.connect(STREAM) as socket:
        print("connected:", await answer(socket, ("success", "error")))
        await socket.send(json.dumps({"action": "auth", "key": key, "secret": secret}))
        authenticated = await answer(socket, ("success", "error"))
        print("auth:", authenticated)
        signed_in = any(item.get("T") == "success" and item.get("msg") == "authenticated" for item in authenticated)
        if not signed_in:
            print("result: sign-in refused")
            return 2
        await socket.send(json.dumps({"action": "subscribe", "statuses": SYMBOLS, "lulds": SYMBOLS}))
        subscribed = await answer(socket, ("subscription", "error"))
        print("subscribe:", subscribed)
        confirmed = [item for item in subscribed if item.get("T") == "subscription"]
        both = bool(confirmed) and all(
            set(SYMBOLS) <= set(item.get(channel, [])) for item in confirmed for channel in ("statuses", "lulds")
        )
        if not both:
            print("result: subscription not confirmed for statuses and lulds")
            return 1
        received = await listen(socket)
        print(f"received in {LISTEN_S}s:", received)
        delivered = [kind for kind in received if kind in ("s", "l")]
        observed = f"delivery observed ({', '.join(delivered)})" if delivered else "delivery not observed"
        print(f"result: subscription confirmed for statuses and lulds, {observed}")
        return 0


def main() -> int:
    key = os.environ.get("APCA_API_KEY_ID", "")
    secret = os.environ.get("APCA_API_SECRET_KEY", "")
    if not key or not secret:
        print("set APCA_API_KEY_ID and APCA_API_SECRET_KEY (the paper key) in the environment", file=sys.stderr)
        return 3
    try:
        return asyncio.run(probe(key, secret))
    except (OSError, TimeoutError, websockets.WebSocketException, ValueError) as error:
        print(f"result: inconclusive ({type(error).__name__})")
        return 4


if __name__ == "__main__":
    sys.exit(main())
