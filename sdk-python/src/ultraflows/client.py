"""Synchronous client for the UltraFlows server.

This mirrors the Rust SDK's ``Client`` method for method: same
endpoints, same JSON shapes, same SCREAMING_SNAKE_CASE statuses.
It is deliberately synchronous: workers and LangGraph graphs are
synchronous code, and a FastAPI endpoint stays non-blocking by
calling it from ``asyncio.to_thread`` (see
``examples/fastapi_langgraph_example.py``).

There is no authentication in this server release; the server binds
localhost-only. Do not point this client at a server exposed to a
network you do not trust.
"""

from __future__ import annotations

from typing import Any

from .errors import ApiError
from .transport import Response, Transport, default_transport
from .types import LeasedRun, RunView, StepView, TaskDef, TriggerOptions


class Client:
    def __init__(
        self,
        base_url: str = "http://127.0.0.1:3000",
        transport: Transport | None = None,
        timeout: float = 30.0,
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self._transport = transport if transport is not None else default_transport(
            timeout=timeout
        )

    def close(self) -> None:
        close = getattr(self._transport, "close", None)
        if callable(close):
            close()

    def __enter__(self) -> Client:  # noqa: PYI034 - returns self at runtime
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    # -- low level ----------------------------------------------------

    def _url(self, path: str) -> str:
        return f"{self.base_url}{path}"

    def _request(
        self, method: str, path: str, json_body: Any = None
    ) -> Any:
        resp: Response = self._transport.request(method, self._url(path), json_body)
        if not 200 <= resp.status < 300:
            raise ApiError(resp.status, resp.text)
        return resp.json

    # -- worker registration / heartbeat ------------------------------

    def register(
        self, worker_id: str, tasks: list[TaskDef]
    ) -> dict[str, Any]:
        """``POST /v1/workers/register`` — upsert task definitions.

        Returns ``{"worker_id", "tasks", "promoted_runs"}``; promoted
        runs are previously ``PENDING_VERSION`` runs of newly
        registered tasks, now runnable.
        """
        return self._request(
            "POST",
            "/v1/workers/register",
            {"worker_id": worker_id, "tasks": [t.to_dict() for t in tasks]},
        )

    def worker_heartbeat(self, worker_id: str) -> None:
        self._request("POST", f"/v1/workers/{worker_id}/heartbeat")

    # -- triggering ----------------------------------------------------

    def trigger(
        self,
        task_id: str,
        options: TriggerOptions | dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        """``POST /v1/tasks/{id}/trigger`` -> ``{"run_id", "status", "created"}``.

        ``created`` is False when an idempotency key replayed an
        earlier trigger: the same ``run_id`` comes back, no second run.
        """
        body = options.to_dict() if isinstance(options, TriggerOptions) else (options or {})
        return self._request("POST", f"/v1/tasks/{task_id}/trigger", body)

    def batch_trigger(
        self,
        task_id: str,
        items: list[TriggerOptions | dict[str, Any]],
    ) -> list[dict[str, Any]]:
        """``POST /v1/tasks/{id}/batch-trigger`` (server cap: 100 items)."""
        body = {
            "items": [
                i.to_dict() if isinstance(i, TriggerOptions) else i for i in items
            ]
        }
        return self._request("POST", f"/v1/tasks/{task_id}/batch-trigger", body)

    # -- runs ----------------------------------------------------------

    def get_run(self, run_id: str) -> RunView:
        return RunView.from_dict(self._request("GET", f"/v1/runs/{run_id}"))

    def list_runs(
        self,
        status: str | None = None,
        task: str | None = None,
        tag: str | None = None,
        from_ts: int | None = None,
        to_ts: int | None = None,
    ) -> list[RunView]:
        from urllib.parse import urlencode

        query = urlencode(
            {k: v for k, v in
             {"status": status, "task": task, "tag": tag,
              "from": from_ts, "to": to_ts}.items() if v is not None}
        )
        path = "/v1/runs" + (f"?{query}" if query else "")
        return [RunView.from_dict(r) for r in self._request("GET", path)]

    def get_events(self, run_id: str) -> list[dict[str, Any]]:
        return self._request("GET", f"/v1/runs/{run_id}/events")

    def get_steps(self, run_id: str) -> list[StepView]:
        return [StepView.from_dict(s) for s in self._request("GET", f"/v1/runs/{run_id}/steps")]

    def get_wait(self, run_id: str, key: str) -> dict[str, Any] | None:
        """``GET /v1/runs/{id}/waits/{key}`` — None when no such wait."""
        try:
            return self._request("GET", f"/v1/runs/{run_id}/waits/{key}")
        except ApiError as exc:
            if exc.status == 404:
                return None
            raise

    def cancel_run(self, run_id: str) -> dict[str, Any]:
        return self._request("POST", f"/v1/runs/{run_id}/cancel")

    def replay_run(self, run_id: str, payload: Any = None) -> dict[str, Any]:
        """Replay a terminal run as a new run (linked via ``source_run_id``)."""
        return self._request(
            "POST", f"/v1/runs/{run_id}/replay", {"payload": payload}
        )

    def reschedule_run(self, run_id: str, delay_ms: int) -> dict[str, Any]:
        """Delayed, not-yet-started (``PENDING``, attempt 0) runs only."""
        return self._request(
            "POST", f"/v1/runs/{run_id}/reschedule", {"delay_ms": delay_ms}
        )

    # -- schedules ------------------------------------------------------

    def create_schedule(
        self,
        task_id: str,
        cron: str,
        timezone: str,
        payload: Any = None,
        deduplication_key: str | None = None,
        external_id: str | None = None,
    ) -> dict[str, Any]:
        body = {
            "task_id": task_id,
            "cron": cron,
            "timezone": timezone,
            "payload": payload if payload is not None else {},
            "deduplication_key": deduplication_key,
            "external_id": external_id,
        }
        return self._request("POST", "/v1/schedules", body)

    def list_schedules(self) -> list[dict[str, Any]]:
        return self._request("GET", "/v1/schedules")

    def update_schedule(
        self,
        schedule_id: str,
        cron: str | None = None,
        timezone: str | None = None,
        payload: Any = None,
        enabled: bool | None = None,
    ) -> dict[str, Any]:
        body = {
            k: v
            for k, v in
            {"cron": cron, "timezone": timezone, "payload": payload,
             "enabled": enabled}.items()
            if v is not None
        }
        return self._request("PATCH", f"/v1/schedules/{schedule_id}", body)

    def delete_schedule(self, schedule_id: str) -> dict[str, Any]:
        return self._request("DELETE", f"/v1/schedules/{schedule_id}")

    # -- waitpoints ------------------------------------------------------

    def complete_waitpoint(self, token: str, output: Any = None) -> dict[str, Any]:
        """``POST /v1/waitpoints/{token}/complete`` -> ``{"ok", "woken_runs"}``."""
        return self._request(
            "POST", f"/v1/waitpoints/{token}/complete",
            {"output": output if output is not None else {}},
        )

    def get_waitpoint(self, token: str) -> dict[str, Any]:
        """``{"completed": bool, "output": ...}`` — output only when completed."""
        return self._request("GET", f"/v1/waitpoints/{token}")

    # -- worker protocol (lease / report) -------------------------------

    def lease(self, worker_id: str, tasks: list[str]) -> LeasedRun | None:
        """``POST /v1/workers/{id}/lease`` — one due run, or None."""
        data = self._request("POST", f"/v1/workers/{worker_id}/lease", {"tasks": tasks})
        if data is None:
            return None
        return LeasedRun.from_dict(data)

    def report_step(self, run_id: str, key: str, result: Any) -> dict[str, Any]:
        """Insert-once: a duplicate report returns the recorded result."""
        return self._request(
            "POST", f"/v1/workers/leases/{run_id}/step",
            {"key": key, "result": result},
        )

    def report_wait(
        self,
        run_id: str,
        key: str,
        kind: str,
        wake_at: str | None = None,
        token: str | None = None,
    ) -> None:
        self._request(
            "POST", f"/v1/workers/leases/{run_id}/wait",
            {"key": key, "kind": kind, "wake_at": wake_at, "token": token},
        )

    def report_complete(self, run_id: str, output: Any) -> None:
        self._request(
            "POST", f"/v1/workers/leases/{run_id}/complete", {"output": output}
        )

    def report_fail(self, run_id: str, worker_id: str, error: str) -> None:
        self._request(
            "POST", f"/v1/workers/leases/{run_id}/fail",
            {"error": error, "worker_id": worker_id},
        )

    def lease_heartbeat(self, run_id: str, worker_id: str) -> None:
        self._request(
            "POST", f"/v1/workers/leases/{run_id}/heartbeat",
            {"worker_id": worker_id},
        )
