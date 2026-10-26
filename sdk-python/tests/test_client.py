"""Client tests against a stub transport — no live server needed.

FakeTransport implements the SDK's transport protocol
(``request(method, url, json_body) -> Response``) and answers from a
table of canned responses, recording every call so tests can assert
the exact endpoint and JSON shape the client sent — the same shapes
the Rust SDK sends.
"""

import pytest

from ultraflows import Client, TriggerOptions
from ultraflows.errors import ApiError
from ultraflows.transport import Response


class FakeTransport:
    def __init__(self, routes):
        """routes: {(method, path): (status, json_body)}; path is the
        URL path after the base URL."""
        self.routes = routes
        self.calls = []

    def request(self, method, url, json_body=None):
        path = url.split("http://test", 1)[-1]
        self.calls.append((method, path, json_body))
        status, body = self.routes.get((method, path), (404, "not found"))
        return Response(status=status, json=body, text=str(body))


def make_client(routes):
    return Client("http://test", transport=FakeTransport(routes))


def test_trigger_posts_snake_case_body_and_returns_run():
    client = make_client(
        {("POST", "/v1/tasks/summarize/trigger"): (200, {"run_id": "r1", "status": "PENDING", "created": True})}
    )
    out = client.trigger(
        "summarize",
        TriggerOptions(payload={"text": "hi"}, idempotency_key="k1", idempotency_scope="run"),
    )
    assert out["run_id"] == "r1"
    method, path, body = client._transport.calls[0]
    assert (method, path) == ("POST", "/v1/tasks/summarize/trigger")
    # Wire is snake_case, and None fields are omitted.
    assert body == {"payload": {"text": "hi"}, "idempotency_key": "k1", "idempotency_scope": "run"}


def test_idempotent_retrigger_returns_same_run_and_created_false():
    client = make_client(
        {("POST", "/v1/tasks/summarize/trigger"): (200, {"run_id": "r1", "status": "PENDING", "created": False})}
    )
    out = client.trigger("summarize", TriggerOptions(idempotency_key="k1"))
    assert out["created"] is False and out["run_id"] == "r1"


def test_batch_trigger_caps_at_server_and_sends_items():
    client = make_client(
        {("POST", "/v1/tasks/t/batch-trigger"): (200, [{"run_id": "a", "status": "PENDING", "created": True}])}
    )
    out = client.batch_trigger("t", [TriggerOptions(payload={"n": 1})])
    assert out[0]["run_id"] == "a"
    assert client._transport.calls[0][2] == {"items": [{"payload": {"n": 1}}]}


def test_get_run_parses_screaming_snake_status():
    client = make_client(
        {
            ("GET", "/v1/runs/r1"): (
                200,
                {"id": "r1", "workflow": "summarize", "payload": {}, "output": {"ok": True},
                 "error": None, "status": "COMPLETED", "attempt": 1, "max_attempts": 5},
            )
        }
    )
    run = client.get_run("r1")
    # The old Rust bug was comparing "Completed"; the wire says COMPLETED.
    assert run.status == "COMPLETED"
    assert run.is_terminal


def test_list_runs_builds_filter_query():
    client = make_client(
        {("GET", "/v1/runs?status=COMPLETED&task=summarize"): (200, [])}
    )
    assert client.list_runs(status="COMPLETED", task="summarize") == []
    assert client._transport.calls[0][1] == "/v1/runs?status=COMPLETED&task=summarize"


def test_api_error_carries_status_and_body():
    client = make_client({("GET", "/v1/runs/nope"): (404, "not found: nope")})
    with pytest.raises(ApiError) as excinfo:
        client.get_run("nope")
    assert excinfo.value.status == 404


def test_get_wait_returns_none_on_404():
    client = make_client({("GET", "/v1/runs/r1/waits/k"): (404, "not found")})
    assert client.get_wait("r1", "k") is None


def test_lease_none_when_no_work():
    client = make_client({("POST", "/v1/workers/w1/lease"): (200, None)})
    assert client.lease("w1", ["summarize"]) is None


def test_lease_returns_run_plus_steps():
    client = make_client(
        {
            ("POST", "/v1/workers/w1/lease"): (
                200,
                {"run": {"id": "r1", "workflow": "summarize", "payload": {"t": "x"},
                        "output": None, "error": None, "status": "RUNNING",
                        "attempt": 2, "max_attempts": 5},
                 "steps": [{"run_id": "r1", "key": "summary", "result": "done"}]},
            )
        }
    )
    leased = client.lease("w1", ["summarize"])
    assert leased.run.attempt == 2
    assert leased.steps[0].key == "summary"


def test_register_sends_task_defs():
    client = make_client(
        {("POST", "/v1/workers/register"): (200, {"worker_id": "w1", "tasks": ["summarize"], "promoted_runs": []})}
    )
    from ultraflows import TaskDef

    out = client.register("w1", [TaskDef(id="summarize", version_hash="v1")])
    assert out["tasks"] == ["summarize"]
    assert client._transport.calls[0][2] == {
        "worker_id": "w1",
        "tasks": [{"id": "summarize", "version_hash": "v1"}],
    }


def test_schedule_crud_endpoints():
    client = make_client(
        {
            ("POST", "/v1/schedules"): (200, {"id": "s1"}),
            ("GET", "/v1/schedules"): (200, [{"id": "s1"}]),
            ("PATCH", "/v1/schedules/s1"): (200, {"id": "s1", "enabled": False}),
            ("DELETE", "/v1/schedules/s1"): (200, {"ok": True}),
        }
    )
    assert client.create_schedule("t", "0 9 * * *", "America/Havana")["id"] == "s1"
    assert len(client.list_schedules()) == 1
    assert client.update_schedule("s1", enabled=False)["enabled"] is False
    assert client.delete_schedule("s1")["ok"] is True


def test_waitpoint_complete_and_get():
    client = make_client(
        {
            ("POST", "/v1/waitpoints/tok/complete"): (200, {"ok": True, "woken_runs": ["r1"]}),
            ("GET", "/v1/waitpoints/tok"): (200, {"completed": True, "output": {"yes": True}}),
        }
    )
    assert client.complete_waitpoint("tok", {"yes": True})["woken_runs"] == ["r1"]
    assert client.get_waitpoint("tok")["output"] == {"yes": True}


def test_cancel_replay_reschedule():
    client = make_client(
        {
            ("POST", "/v1/runs/r1/cancel"): (200, {"ok": True}),
            ("POST", "/v1/runs/r1/replay"): (200, {"run_id": "r2", "status": "PENDING"}),
            ("POST", "/v1/runs/r1/reschedule"): (200, {"ok": True}),
        }
    )
    assert client.cancel_run("r1")["ok"] is True
    assert client.replay_run("r1")["run_id"] == "r2"
    assert client.reschedule_run("r1", 5000)["ok"] is True
    assert client._transport.calls[2][2] == {"delay_ms": 5000}
