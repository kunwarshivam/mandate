"""One HTTPS transport for both vendors: urllib, JSON, numbers decoded as Decimal, never float."""

import json
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Callable
from decimal import Decimal

Transport = Callable[[str, str, dict[str, str], bytes | None], tuple[int, str]]


class HttpError(Exception):
    def __init__(self, status: int, url: str, body: str):
        super().__init__(f"HTTP {status} from {url.split('?')[0]}: {body[:300]}")
        self.status = status


def urllib_transport(method: str, url: str, headers: dict[str, str], body: bytes | None) -> tuple[int, str]:
    request = urllib.request.Request(url, data=body, headers=headers, method=method)
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.status, response.read().decode("utf-8")
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode("utf-8", errors="replace")


def decode(text: str) -> object:
    return json.loads(text, parse_float=Decimal)


def request_json(
    transport: Transport,
    method: str,
    url: str,
    headers: dict[str, str],
    params: dict[str, str] | None = None,
    body: object | None = None,
) -> object:
    if params:
        url = f"{url}?{urllib.parse.urlencode(params)}"
    payload = None
    if body is not None:
        payload = json.dumps(body, default=str).encode("utf-8")
        headers = {**headers, "Content-Type": "application/json"}
    status, text = transport(method, url, headers, payload)
    if status >= 400:
        raise HttpError(status, url, text)
    return decode(text)
