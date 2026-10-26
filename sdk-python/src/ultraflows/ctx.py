"""Execution context for a leased run — the durability primitive.

``RemoteCtx`` mirrors the Rust SDK's ``RemoteCtx`` and the embedded
``Ctx`` model:

- ``step(key, fn)`` is journaled. On first execution it runs ``fn``,
  reports the result (insert-once on the server), and returns the
  reported result — so a racing duplicate report's recorded value
  wins. On re-execution after a wait, retry, or crash, the recorded
  result is returned WITHOUT calling ``fn`` again. That is what makes
  an expensive step (an LLM call, a payment, an email) safe to replay:
  the model is not re-called, the payment is not re-sent.
- ``sleep`` / ``wait_until`` / ``wait_for_token`` /
  ``trigger_and_wait`` report a wait to the server and then raise
  :class:`~ultraflows.errors.Suspended`. That is not a failure: the
  worker catches it, the run sits in ``WAITING`` costing no worker
  slot, and a later lease re-enters the task function from the top —
  replaying recorded steps and continuing past the wait that fired.
  (This is why task functions must be deterministic between step
  boundaries: they re-run, but their steps do not re-execute.)

Timer subtlety, copied from the Rust SDK: a timer wait that already
fired is a no-op on re-execution. The run is only re-leased once its
wake time passed, so re-suspending on the same key would loop
forever. ``sleep``/``wait_until`` therefore check the persisted wait
first via ``GET /v1/runs/{id}/waits/{key}``.
"""

from __future__ import annotations

from collections.abc import Callable
from datetime import datetime, timedelta, timezone
from typing import Any

from .client import Client
from .errors import RunFailed, Suspended
from .types import (
    STATUS_CANCELLED,
    STATUS_COMPLETED,
    STATUS_EXPIRED,
    STATUS_FAILED,
    StepView,
)


def _to_rfc3339(dt: datetime) -> str:
    """UTC RFC 3339 (``...Z``), as the server's chrono parser expects."""
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return dt.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


def _parse_rfc3339(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


class RemoteCtx:
    def __init__(
        self,
        client: Client,
        run_id: str,
        steps: list[StepView] | None = None,
        attempt: int = 1,
    ) -> None:
        self.client = client
        self.run_id = run_id
        self.attempt = attempt
        self._steps: dict[str, Any] = {s.key: s.result for s in (steps or [])}

    # -- journaled steps -------------------------------------------------

    def step(self, key: str, fn: Callable[[], Any]) -> Any:
        """Run ``fn`` once, forever: recorded results replay without re-running."""
        if key in self._steps:
            return self._steps[key]
        value = fn()
        reported = self.client.report_step(self.run_id, key, value)
        result = reported.get("result", value)
        self._steps[key] = result
        return result

    # -- waits ------------------------------------------------------------

    def _suspend(
        self, key: str, kind: str, wake_at: str | None = None, token: str | None = None
    ) -> None:
        self.client.report_wait(self.run_id, key, kind, wake_at=wake_at, token=token)
        raise Suspended(key)

    def _timer_elapsed(self, key: str) -> bool:
        wait = self.client.get_wait(self.run_id, key)
        if not wait:
            return False
        wake_at = wait.get("wake_at")
        if not wake_at:
            return False
        try:
            return _parse_rfc3339(wake_at) <= datetime.now(timezone.utc)
        except ValueError:
            return False

    def sleep(self, key: str, seconds: float) -> None:
        """Suspend for ``seconds``; a no-op when the timer already fired."""
        if self._timer_elapsed(key):
            return
        wake_at = _to_rfc3339(datetime.now(timezone.utc) + timedelta(seconds=seconds))
        self._suspend(key, "timer", wake_at=wake_at)

    def wait_until(self, key: str, when: datetime) -> None:
        """Suspend until a clock time; a no-op when that time already passed."""
        if self._timer_elapsed(key):
            return
        self._suspend(key, "timer", wake_at=_to_rfc3339(when))

    def wait_for_token(self, key: str, token: str) -> Any:
        """Wait for an external approval token.

        If the token was already completed, its output is returned
        immediately instead of suspending.
        """
        wp = self.client.get_waitpoint(token)
        if wp.get("completed"):
            return wp.get("output", {})
        self._suspend(key, "approval", token=token)

    def trigger_and_wait(self, key: str, task_id: str, payload: Any = None) -> Any:
        """Trigger a child run and suspend until it completes.

        On re-execution the recorded child id is reused (no duplicate
        child) and its output returned once it is ``COMPLETED``. A
        child ending ``FAILED``/``CANCELLED``/``EXPIRED`` raises
        :class:`~ultraflows.errors.RunFailed`.
        """
        wait = self.client.get_wait(self.run_id, key)
        child_id = wait.get("token") if wait else None
        if child_id:
            child = self.client.get_run(child_id)
            if child.status == STATUS_COMPLETED:
                return child.output if child.output is not None else {}
            if child.status in (STATUS_FAILED, STATUS_CANCELLED, STATUS_EXPIRED):
                raise RunFailed(
                    f"child run {child_id} ended as {child.status}"
                )
            # Still running/waiting: re-suspend on it.
            self._suspend(key, "child", token=child_id)
        child = self.client.trigger(task_id, {"payload": payload if payload is not None else {}})
        self._suspend(key, "child", token=child["run_id"])
