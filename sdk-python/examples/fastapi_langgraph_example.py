"""FastAPI + LangGraph patterns for UltraFlows (Python SDK).

This file is runnable as-is WITHOUT FastAPI or LangGraph installed:
it defines the worker task and prints what each pattern does. With the
optional packages installed it also builds the real FastAPI app and
the real LangGraph graph.

To run it for real::

    # From sdk-python/:
    uv pip install fastapi uvicorn langgraph
    # (or: pip install fastapi uvicorn langgraph)
    # Then start the UltraFlows server first (from the repo root):
    #   ULTRAFLOW_DB=/tmp/uf.db PORT=3000 cargo run --bin ultraflows-server
    # And in another terminal, run this file's worker:
    #   python examples/fastapi_langgraph_example.py --worker

The two patterns Roger will actually use:

(a) FASTAPI TRIGGERS, WORKER EXECUTES (the normal pattern).
    A FastAPI endpoint does NOT run the job — it triggers an
    UltraFlows run and returns the run id immediately. A separate
    worker process (this file, ``--worker``) leases and executes it.
    The endpoint stays async and non-blocking because the synchronous
    SDK call runs in ``asyncio.to_thread``.

(b) LANGGRAPH RUNS *INSIDE* A WORKER TASK (the durable-LLM pattern).
    The LangGraph graph executes inside the task function, and every
    expensive node (LLM call, tool call, retrieval) is wrapped in
    ``ctx.step()``. If the worker crashes mid-graph, the next lease
    re-enters the task, replays recorded steps WITHOUT re-calling the
    model, and continues. Without the ``ctx.step`` wrapper, a crash
    would re-run the whole graph and re-pay for every LLM call.

ALTERNATIVE PATTERN (note, not the default): a LangGraph node can
trigger a *child* UltraFlows run and wait on it instead of running
the work inline. Two ways to wait:

- From inside a worker task: ``ctx.trigger_and_wait("child", "task",
  payload)`` — suspends the parent run (no worker slot) until the
  child completes, then returns the child's output. This is the
  durable version: a crash between trigger and completion loses
  nothing, because the child id is journaled in the wait record.
- From outside a worker (e.g. a plain LangGraph node in your own
  service): ``client.trigger(...)`` then poll ``client.get_run(id)``
  until ``status`` is a terminal one (``COMPLETED``/``FAILED``/
  ``CANCELLED``/``EXPIRED``). Polling is simple but NOT durable:
  if your service crashes while polling, the polling state is lost
  (the child run itself is fine — it keeps running on its worker).
  Prefer ``trigger_and_wait`` inside a task whenever the wait itself
  must survive a crash.

Security note: this server release has no auth and binds
localhost-only. These examples assume the server runs on the same
host as the FastAPI service and the worker.
"""

from __future__ import annotations

import asyncio
import os
import sys

# Allow running from the examples/ directory without installing the SDK.
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

from ultraflows import Client, TriggerOptions, Worker

BASE_URL = os.environ.get("ULTRAFLOWS_URL", "http://127.0.0.1:3000")
client = Client(BASE_URL)
worker = Worker(client, worker_id="py-example-worker")


# ---------------------------------------------------------------------------
# The shared task: a small LangGraph graph, durably executed.
# ---------------------------------------------------------------------------
def build_graph():
    """Build the LangGraph graph (only when langgraph is installed).

    The graph is deliberately tiny — two nodes — because the point
    here is durability, not graph cleverness. Replace the node bodies
    with your real LLM/tool calls.
    """
    try:
        from langgraph.graph import END, StateGraph
    except ImportError as exc:  # pragma: no cover
        raise RuntimeError("install langgraph to build the graph") from exc

    def draft(state: dict) -> dict:
        # In real code: call your model here.
        return {"draft": f"draft of: {state['text']}"}

    def polish(state: dict) -> dict:
        # In real code: call your model/tool here.
        return {"final": state["draft"].upper()}

    graph = StateGraph(dict)
    graph.add_node("draft", draft)
    graph.add_node("polish", polish)
    graph.set_entry_point("draft")
    graph.add_edge("draft", "polish")
    graph.add_edge("polish", END)
    return graph.compile()


@worker.task("summarize-with-graph", version_hash="v1")
def summarize_with_graph(ctx, payload):
    """Worker task: run the graph, with each node inside ctx.step().

    Why the wrapper matters: on re-execution after a crash or a wait,
    ``ctx.step("draft", ...)`` returns the recorded draft WITHOUT
    calling the model again. The graph still "runs" top to bottom,
    but recorded nodes are replays, not re-executions.
    """
    graph = build_graph()
    draft = ctx.step("draft", lambda: graph.invoke({"text": payload["text"]})["draft"])
    final = ctx.step("polish", lambda: graph.invoke({"text": draft, "draft": draft})["final"])
    return {"summary": final}


# ---------------------------------------------------------------------------
# (a) FastAPI: trigger and return the run id, without blocking.
# ---------------------------------------------------------------------------
try:
    from fastapi import FastAPI

    app = FastAPI(title="UltraFlows trigger API")

    @app.post("/summaries")
    async def create_summary(text: str):
        # Synchronous SDK call, off the event loop: the endpoint
        # returns as soon as the run is created, it does not wait
        # for the (possibly minutes-long) work to finish.
        run = await asyncio.to_thread(
            client.trigger,
            "summarize-with-graph",
            TriggerOptions(payload={"text": text}, idempotency_key=f"summary:{text[:32]}"),
        )
        return {"run_id": run["run_id"], "status": run["status"]}

    @app.get("/summaries/{run_id}")
    async def get_summary(run_id: str):
        run = await asyncio.to_thread(client.get_run, run_id)
        return {"run_id": run.id, "status": run.status, "output": run.output}

except ImportError:  # pragma: no cover - exercised when fastapi is absent
    app = None


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------
def main() -> None:
    if "--worker" in sys.argv:
        print(f"Registering tasks on {BASE_URL} and driving the worker...")
        print("Registered:", worker.register())
        print("Leasing work (Ctrl-C to stop)...")
        worker.drive()
        return
    print("UltraFlows FastAPI + LangGraph example")
    print(f"  Server: {BASE_URL}")
    print("  Task registered in code: summarize-with-graph")
    print(f"  FastAPI app built: {app is not None} (pip install fastapi to enable)")
    try:
        build_graph()
        print("  LangGraph graph builds: True")
    except RuntimeError:
        print("  LangGraph graph builds: False (pip install langgraph to enable)")
    print()
    print("  Run the worker:  python examples/fastapi_langgraph_example.py --worker")
    print("  Serve the API:   uvicorn examples.fastapi_langgraph_example:app --reload")
    print("  (Start the UltraFlows server first; see the module docstring.)")


if __name__ == "__main__":
    main()
