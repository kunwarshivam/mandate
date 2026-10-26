"""Ask Alpaca's stock stream whether the paper data plan carries trading statuses and LULD bands.

Trading spec §4.4 needs the instrument's trading status and LULD bands. Alpaca's stock stream names
both channels, `statuses` and `lulds`, but its documentation does not say whether the free `iex`
feed carries them. This probe asks the stream once and prints only what the stream answered to
the authentication and the subscription: message types, codes, and the channels the subscription
was confirmed for. It never prints the key, the secret, or any market data, and it places no
order: it talks only to the market-data stream host.

Run it once, from a machine that holds the founder's Alpaca paper key in its environment:

    APCA_API_KEY_ID=... APCA_API_SECRET_KEY=... \
        uv run --no-project --with 'websockets==15.0.1' python scripts/alpaca-status-probe.py

`websockets` is fetched for this one run only, into uv's cache; the probe adds no dependency to
any workspace project. The key is read from the environment and nowhere else.
"""

import asyncio
import json
import os
import sys

import websockets

STREAM = "wss://stream.data.alpaca.markets/v2/iex"
SYMBOL = "SPY"
SAFE_KEYS = ("T", "msg", "code", "statuses", "lulds", "trades", "quotes", "bars")


def safe(message: dict) -> dict:
    """Only the members that describe the answer: never a price, a time, or anything sent."""
    return {key: message[key] for key in SAFE_KEYS if key in message}


async def answer(socket) -> list[dict]:
    frame = await asyncio.wait_for(socket.recv(), timeout=15)
    parsed = json.loads(frame)
    return [safe(item) for item in (parsed if isinstance(parsed, list) else [parsed])]


async def probe(key: str, secret: str) -> int:
    async with websockets.connect(STREAM) as socket:
        print("connected:", await answer(socket))
        await socket.send(json.dumps({"action": "auth", "key": key, "secret": secret}))
        authenticated = await answer(socket)
        print("auth:", authenticated)
        if not any(item.get("T") == "success" and item.get("msg") == "authenticated" for item in authenticated):
            return 2
        await socket.send(json.dumps({"action": "subscribe", "statuses": [SYMBOL], "lulds": [SYMBOL]}))
        subscribed = await answer(socket)
        print("subscribe:", subscribed)
        confirmed = [item for item in subscribed if item.get("T") == "subscription"]
        carried = bool(confirmed) and all(SYMBOL in item.get(channel, []) for item in confirmed for channel in ("statuses", "lulds"))
        print("result:", "iex carries statuses and lulds" if carried else "iex does not confirm statuses and lulds")
        return 0 if carried else 1


def main() -> int:
    key = os.environ.get("APCA_API_KEY_ID", "")
    secret = os.environ.get("APCA_API_SECRET_KEY", "")
    if not key or not secret:
        print("set APCA_API_KEY_ID and APCA_API_SECRET_KEY (the paper key) in the environment", file=sys.stderr)
        return 3
    return asyncio.run(probe(key, secret))


if __name__ == "__main__":
    sys.exit(main())
