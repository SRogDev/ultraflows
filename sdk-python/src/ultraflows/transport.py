"""HTTP transport for the SDK.

The SDK talks to the server over plain HTTP/JSON. ``Client`` accepts
any object with a ``request(method, url, json_body) -> Response``
method, so tests can inject a fake without a live server (see
``tests/`` for ``FakeTransport``).

Default transport selection, in order:

1. ``httpx`` if installed (declared in ``pyproject.toml``) — the
   natural choice for FastAPI/LangGraph users, who already live in
   that ecosystem.
2. Otherwise a stdlib ``urllib`` transport, so the SDK still works in
   a bare environment. It is slower to configure (no connection
   pooling) but speaks exactly the same protocol.

Both transports return :class:`Response` with ``status`` and a parsed
``json`` body (``None`` for empty bodies), and raise
:class:`~ultraflows.errors.TransportError` when no answer arrives.
"""

from __future__ import annotations

import json
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Any, Protocol

from .errors import TransportError


@dataclass
class Response:
    status: int
    json: Any  # parsed body, or None when the body is empty
    text: str = ""


class Transport(Protocol):
    def request(
        self, method: str, url: str, json_body: Any = None
    ) -> Response: ...


class HttpxTransport:
    """Transport backed by ``httpx`` (connection pooling, timeouts)."""

    def __init__(self, timeout: float = 30.0) -> None:
        try:
            import httpx
        except ImportError as exc:  # pragma: no cover - exercised by fallback
            raise TransportError(
                "httpx is not installed; install it or pass a custom transport"
            ) from exc
        self._client = httpx.Client(timeout=timeout)

    def request(
        self, method: str, url: str, json_body: Any = None
    ) -> Response:
        import httpx

        try:
            resp = self._client.request(method, url, json=json_body)
        except httpx.HTTPError as exc:
            raise TransportError(str(exc)) from exc
        try:
            body = resp.json() if resp.content else None
        except ValueError:
            body = None
        return Response(status=resp.status_code, json=body, text=resp.text)

    def close(self) -> None:
        self._client.close()


class UrllibTransport:
    """Stdlib-only fallback transport (no third-party dependency)."""

    def __init__(self, timeout: float = 30.0) -> None:
        self._timeout = timeout

    def request(
        self, method: str, url: str, json_body: Any = None
    ) -> Response:
        data = None
        headers = {"Content-Type": "application/json"}
        if json_body is not None:
            data = json.dumps(json_body).encode("utf-8")
        req = urllib.request.Request(url, data=data, headers=headers, method=method)
        try:
            with urllib.request.urlopen(req, timeout=self._timeout) as resp:
                raw = resp.read().decode("utf-8")
                status = resp.status
        except urllib.error.HTTPError as exc:
            raw = exc.read().decode("utf-8")
            status = exc.code
        except urllib.error.URLError as exc:
            raise TransportError(str(exc)) from exc
        try:
            body = json.loads(raw) if raw.strip() else None
        except ValueError:
            body = None
        return Response(status=status, json=body, text=raw)


def default_transport(timeout: float = 30.0) -> Transport:
    """Prefer httpx; fall back to urllib when httpx is unavailable."""
    try:
        return HttpxTransport(timeout=timeout)
    except TransportError:
        return UrllibTransport(timeout=timeout)
