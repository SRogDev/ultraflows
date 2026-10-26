"""RemoteCtx tests: journal/replay and suspend semantics.

The client here is a recording stub (not a transport): it answers
``report_step`` / ``get_wait`` / ``get_waitpoint`` / ``trigger`` /
``get_run`` from in-memory state, so these tests prove the context
logic itself — the part that must never re-run an expensive step.
"""

import pytest

from ultraflows.ctx import RemoteCtx
from ultraflows.errors import RunFailed, Suspended
from ultraflows.types import StepView


class StubClient:
    def __init__(self):
        self.reported_steps = {}  # (run_id, key) -> result
        self.waits = {}  # (run_id, key) -> wait dict
        self.waitpoints = {}  # token -> {"completed": bool, "output": ...}
        self.runs = {}  # run_id -> RunView-like stub
        self.triggered = []
        self.report_wait_calls = []

    def report_step(self, run_id, key, result):
        # Insert-once: first result wins, like the server.
        existing = self.reported_steps.setdefault((run_id, key), result)
        return {"recorded": existing is result, "result": existing}

    def report_wait(self, run_id, key, kind, wake_at=None, token=None):
        self.waits[(run_id, key)] = {"key": key, "kind": kind, "wake_at": wake_at, "token": token}
        self.report_wait_calls.append((key, kind, token))

    def get_wait(self, run_id, key):
        return self.waits.get((run_id, key))

    def get_waitpoint(self, token):
        return self.waitpoints.get(token, {"completed": False})

    def trigger(self, task_id, options):
        self.triggered.append((task_id, options))
        return {"run_id": f"child-{len(self.triggered)}", "status": "PENDING", "created": True}

    def get_run(self, run_id):
        return self.runs[run_id]


def test_step_recorded_result_returned_without_calling_fn():
    stub = StubClient()
    steps = [StepView(run_id="r1", key="summary", result="already-done")]
    ctx = RemoteCtx(stub, "r1", steps, attempt=2)
    calls = []
    result = ctx.step("summary", lambda: calls.append(1) or "recomputed")
    assert result == "already-done"
    assert calls == [], "recorded step must NOT re-execute the function"
    assert stub.reported_steps == {}, "no report for a replayed step"


def test_step_runs_once_and_reports_result():
    stub = StubClient()
    ctx = RemoteCtx(stub, "r1", [], attempt=1)
    assert ctx.step("count", lambda: 42) == 42
    assert stub.reported_steps[("r1", "count")] == 42
    # Second call in the same attempt replays from the local journal.
    assert ctx.step("count", lambda: 99) == 42


def test_step_duplicate_report_returns_recorded_winner():
    stub = StubClient()
    stub.reported_steps[("r1", "k")] = "first"
    ctx = RemoteCtx(stub, "r1", [], attempt=1)
    assert ctx.step("k", lambda: "second") == "first"


def test_sleep_reports_timer_wait_and_suspends():
    stub = StubClient()
    ctx = RemoteCtx(stub, "r1", [], attempt=1)
    with pytest.raises(Suspended) as excinfo:
        ctx.sleep("nap", 5)
    assert excinfo.value.key == "nap"
    key, kind, _token = stub.report_wait_calls[0]
    assert (key, kind) == ("nap", "timer")
    assert stub.waits[("r1", "nap")]["wake_at"] is not None


def test_sleep_noop_when_timer_already_fired():
    stub = StubClient()
    # A wake_at in the past means the run was re-leased after firing.
    stub.waits[("r1", "nap")] = {"key": "nap", "kind": "timer", "wake_at": "2020-01-01T00:00:00Z", "token": None}
    ctx = RemoteCtx(stub, "r1", [], attempt=2)
    ctx.sleep("nap", 5)  # must NOT raise Suspended again
    assert stub.report_wait_calls == []


def test_wait_for_token_returns_output_when_completed():
    stub = StubClient()
    stub.waitpoints["tok-1"] = {"completed": True, "output": {"approved": True}}
    ctx = RemoteCtx(stub, "r1", [], attempt=1)
    assert ctx.wait_for_token("approval", "tok-1") == {"approved": True}
    assert stub.report_wait_calls == []


def test_wait_for_token_suspends_when_pending():
    stub = StubClient()
    ctx = RemoteCtx(stub, "r1", [], attempt=1)
    with pytest.raises(Suspended):
        ctx.wait_for_token("approval", "tok-1")
    assert stub.report_wait_calls[0] == ("approval", "approval", "tok-1")


def test_trigger_and_wait_triggers_child_then_suspends():
    stub = StubClient()
    ctx = RemoteCtx(stub, "r1", [], attempt=1)
    with pytest.raises(Suspended):
        ctx.trigger_and_wait("child", "summarize", {"text": "hi"})
    assert stub.triggered == [("summarize", {"payload": {"text": "hi"}})]
    assert stub.report_wait_calls[0][1] == "child"
    assert stub.report_wait_calls[0][2] == "child-1"


def test_trigger_and_wait_reuses_child_and_returns_output():
    from ultraflows.types import RunView

    stub = StubClient()
    stub.waits[("r1", "child")] = {"key": "child", "kind": "child", "wake_at": None, "token": "child-1"}
    stub.runs["child-1"] = RunView(
        id="child-1", workflow="summarize", payload={}, output={"summary": "ok"},
        error=None, status="COMPLETED", attempt=1, max_attempts=5,
    )
    ctx = RemoteCtx(stub, "r1", [], attempt=2)
    assert ctx.trigger_and_wait("child", "summarize", {}) == {"summary": "ok"}
    assert stub.triggered == [], "no duplicate child on re-execution"


def test_trigger_and_wait_failed_child_raises_run_failed():
    from ultraflows.types import RunView

    stub = StubClient()
    stub.waits[("r1", "child")] = {"key": "child", "kind": "child", "wake_at": None, "token": "child-1"}
    stub.runs["child-1"] = RunView(
        id="child-1", workflow="summarize", payload={}, output=None,
        error="boom", status="FAILED", attempt=3, max_attempts=3,
    )
    ctx = RemoteCtx(stub, "r1", [], attempt=2)
    with pytest.raises(RunFailed):
        ctx.trigger_and_wait("child", "summarize", {})
