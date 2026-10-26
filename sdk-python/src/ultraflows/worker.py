"""Worker: lease runs from the server and execute task functions.

A worker is the other half of the protocol: the server never runs
task code itself. It leases a due run (concurrency caps enforced at
claim time on the server), hands the worker the run plus its
journaled steps, and the worker executes the task function with a
``RemoteCtx`` and reports the outcome:

- normal return -> ``report_complete`` (run ``COMPLETED``)
- :class:`~ultraflows.errors.Suspended` -> nothing more to do; the
  wait was already reported by ``RemoteCtx`` and the run is
  ``WAITING`` on the server
- any other exception -> ``report_fail``; the server retries per the
  task's retry policy (or marks the run ``FAILED`` when exhausted)

Leases last 30 s (server constant); while a task runs, a heartbeat
thread renews the lease every 10 s, mirroring the Rust SDK. If a
worker dies, its lease expires and another worker reclaims the run —
attempts are at-least-once, recorded steps exactly-once.
"""

from __future__ import annotations

import threading
import time
import uuid
from collections.abc import Callable
from typing import Any

from .client import Client
from .ctx import RemoteCtx
from .errors import Suspended
from .types import TaskDef

TaskFn = Callable[[RemoteCtx, Any], Any]

LEASE_HEARTBEAT_SECONDS = 10.0


class Worker:
    def __init__(
        self,
        client: Client,
        worker_id: str | None = None,
        heartbeat_seconds: float = LEASE_HEARTBEAT_SECONDS,
    ) -> None:
        self.client = client
        self.worker_id = worker_id or f"py-worker-{uuid.uuid4()}"
        self._heartbeat_seconds = heartbeat_seconds
        self._defs: list[TaskDef] = []
        self._tasks: dict[str, TaskFn] = {}

    # -- registration ------------------------------------------------------

    def register_task(
        self,
        task_id: str,
        fn: TaskFn,
        version_hash: str = "v1",
        **def_kwargs: Any,
    ) -> TaskFn:
        """Register ``fn`` under ``task_id``; returns ``fn`` unchanged."""
        self._tasks[task_id] = fn
        self._defs.append(TaskDef(id=task_id, version_hash=version_hash, **def_kwargs))
        return fn

    def task(self, task_id: str, version_hash: str = "v1", **def_kwargs: Any):
        """Decorator form of :meth:`register_task`.

        .. code-block:: python

            @worker.task("summarize")
            def summarize(ctx, payload):
                return ctx.step("summary", lambda: llm(payload["text"]))
        """

        def decorator(fn: TaskFn) -> TaskFn:
            return self.register_task(task_id, fn, version_hash, **def_kwargs)

        return decorator

    def task_ids(self) -> list[str]:
        return list(self._tasks.keys())

    def register(self) -> dict[str, Any]:
        """Send task definitions to the server (upsert + promote)."""
        return self.client.register(self.worker_id, self._defs)

    # -- execution ----------------------------------------------------------

    def _heartbeat_loop(self, run_id: str, stop: threading.Event) -> None:
        while not stop.wait(self._heartbeat_seconds):
            try:
                self.client.lease_heartbeat(run_id, self.worker_id)
            except Exception:  # noqa: BLE001, S110 - heartbeat failure must not kill the task
                # A missed heartbeat only risks lease expiry, which the
                # server handles by reclaiming the run; never crash the
                # task over it.
                pass

    def run_once(self) -> str | None:
        """Lease one run and execute it. Returns the run id, or None
        when no work was available."""
        leased = self.client.lease(self.worker_id, self.task_ids())
        if leased is None:
            return None
        run_id = leased.run.id
        fn = self._tasks.get(leased.run.workflow)
        if fn is None:
            # Should not happen (lease only returns declared tasks).
            # Fail it so it retries instead of wedging the queue.
            self.client.report_fail(run_id, self.worker_id, "worker has no such task")
            return run_id
        ctx = RemoteCtx(self.client, run_id, leased.steps, leased.run.attempt)
        stop = threading.Event()
        hb = threading.Thread(
            target=self._heartbeat_loop, args=(run_id, stop), daemon=True
        )
        hb.start()
        try:
            output = fn(ctx, leased.run.payload)
        except Suspended:
            pass  # Wait already reported; the run is WAITING.
        except Exception as exc:  # noqa: BLE001 - worker must report, not crash
            self.client.report_fail(run_id, self.worker_id, str(exc))
        else:
            self.client.report_complete(run_id, output)
        finally:
            stop.set()
        return run_id

    def drive(self, poll_interval: float = 0.5, max_iterations: int | None = None) -> None:
        """Poll for leased work forever (or ``max_iterations`` loops).

        Errors in a single iteration are swallowed after a poll delay,
        mirroring the Rust SDK's ``drive``: one bad lease must not kill
        the worker process.
        """
        iterations = 0
        while max_iterations is None or iterations < max_iterations:
            iterations += 1
            try:
                worked = self.run_once()
            except Exception:  # noqa: BLE001 - one bad lease must not kill the worker
                worked = None
            if worked is None:
                time.sleep(poll_interval)
