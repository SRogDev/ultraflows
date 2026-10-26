"""UltraFlows Python SDK — durable workflows for FastAPI and LangGraph apps.

Quickstart::

    from ultraflows import Client, TriggerOptions, Worker

    client = Client("http://127.0.0.1:3000")
    run = client.trigger("summarize", TriggerOptions(payload={"text": "..."}))
    print(run["run_id"])

    worker = Worker(client, worker_id="py-worker-1")

    @worker.task("summarize")
    def summarize(ctx, payload):
        return ctx.step("summary", lambda: expensive_llm_call(payload["text"]))

    worker.register()
    worker.drive()

See ``sdk-python/README.md`` for the full guide, and
``examples/fastapi_langgraph_example.py`` for the FastAPI + LangGraph
patterns. The protocol is UltraFlows' own (Trigger-shaped, NOT
wire-compatible with Trigger.dev's hosted API); the server binds
localhost-only and has no auth in this release.
"""

from .client import Client
from .ctx import RemoteCtx
from .errors import (
    ApiError,
    RunFailed,
    Suspended,
    TransportError,
    UltraflowsError,
)
from .types import (
    STATUS_CANCELLED,
    STATUS_COMPLETED,
    STATUS_EXPIRED,
    STATUS_FAILED,
    STATUS_PENDING,
    STATUS_PENDING_VERSION,
    STATUS_RUNNING,
    STATUS_WAITING,
    LeasedRun,
    RetryPolicy,
    RunView,
    StepView,
    TaskDef,
    TriggerOptions,
)
from .worker import Worker

__all__ = [
    "STATUS_CANCELLED",
    "STATUS_COMPLETED",
    "STATUS_EXPIRED",
    "STATUS_FAILED",
    "STATUS_PENDING",
    "STATUS_PENDING_VERSION",
    "STATUS_RUNNING",
    "STATUS_WAITING",
    "ApiError",
    "Client",
    "LeasedRun",
    "RemoteCtx",
    "RetryPolicy",
    "RunFailed",
    "RunView",
    "StepView",
    "Suspended",
    "TaskDef",
    "TransportError",
    "TriggerOptions",
    "UltraflowsError",
    "Worker",
]
