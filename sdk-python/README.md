# UltraFlows Python SDK

Python client + worker for the UltraFlows server — the mechanism for
Python apps (FastAPI services, LangGraph graphs) to send jobs to
UltraFlows and execute them durably. It mirrors the Rust SDK
(`crates/ultraflows-sdk`) method for method: same endpoints, same
JSON shapes, same `SCREAMING_SNAKE_CASE` statuses.

UltraFlows speaks its **own protocol** (Trigger-shaped, not
wire-compatible with Trigger.dev's hosted API). The server binds
localhost-only and has **no auth in this release** — do not expose it
to a network you do not trust.

## Install

```bash
# From sdk-python/
uv sync                 # creates .venv with httpx + dev tools
uv pip install -e .     # or: pip install -e .
```

Requires Python >= 3.10. The only runtime dependency is
[`httpx`](https://www.python-httpx.org/) — the natural choice for
FastAPI/LangGraph users. If `httpx` is unavailable at import time,
the SDK falls back to a stdlib `urllib` transport with the same
protocol (no connection pooling, same requests); see
`src/ultraflows/transport.py`.

## Quickstart

### Trigger from anywhere (e.g. a FastAPI endpoint)

```python
from ultraflows import Client, TriggerOptions

client = Client("http://127.0.0.1:3000")
run = client.trigger("summarize", TriggerOptions(payload={"text": "..."}))
print(run["run_id"], run["status"])  # created=True on first trigger
# Same idempotency key -> same run id, created=False, no second run:
run2 = client.trigger("summarize", TriggerOptions(
    payload={"text": "..."}, idempotency_key="sum-1", idempotency_scope="run"))
```

In an async FastAPI endpoint, keep the endpoint non-blocking with
`await asyncio.to_thread(client.trigger, ...)` — see
`examples/fastapi_langgraph_example.py`.

### Worker in 20 lines

```python
from ultraflows import Client, Worker

worker = Worker(Client("http://127.0.0.1:3000"), worker_id="py-worker-1")

@worker.task("summarize")
def summarize(ctx, payload):
    # step() is journaled: on replay after a crash/wait, the recorded
    # result returns WITHOUT re-running the function (no second LLM call).
    summary = ctx.step("summary", lambda: llm(payload["text"]))
    ctx.sleep("cooldown", 60)  # suspends; resumes after the timer fires
    return {"summary": summary}

worker.register()
worker.drive()  # leases, heartbeats (10 s), executes, reports
```

## API reference

### `Client(base_url="http://127.0.0.1:3000", transport=None, timeout=30.0)`

Synchronous. Pass `transport` (any object with
`request(method, url, json_body) -> Response`) to stub it in tests;
otherwise httpx is used, or urllib as fallback. All methods raise
`ApiError(status, body)` on non-2xx and `TransportError` when no
answer arrives.

| Method | Endpoint | Notes |
|---|---|---|
| `register(worker_id, tasks)` | `POST /v1/workers/register` | Upserts `TaskDef`s; returns `{"worker_id", "tasks", "promoted_runs"}` |
| `worker_heartbeat(worker_id)` | `POST /v1/workers/{id}/heartbeat` | |
| `trigger(task_id, options)` | `POST /v1/tasks/{id}/trigger` | `options`: `TriggerOptions` or dict; returns `{"run_id", "status", "created"}` |
| `batch_trigger(task_id, items)` | `POST /v1/tasks/{id}/batch-trigger` | Server cap: 100 items |
| `get_run(run_id)` / `list_runs(status, task, tag, from_ts, to_ts)` | `GET /v1/runs[/...]` | Returns `RunView` (status is `SCREAMING_SNAKE_CASE`) |
| `get_events(run_id)` / `get_steps(run_id)` | `GET /v1/runs/{id}/events`, `/steps` | Steps are `StepView` |
| `get_wait(run_id, key)` | `GET /v1/runs/{id}/waits/{key}` | `None` on 404 |
| `cancel_run(run_id)` | `POST /v1/runs/{id}/cancel` | |
| `replay_run(run_id, payload=None)` | `POST /v1/runs/{id}/replay` | Terminal runs only; new run linked via `source_run_id` |
| `reschedule_run(run_id, delay_ms)` | `POST /v1/runs/{id}/reschedule` | Delayed, not-yet-started (`PENDING`, attempt 0) only |
| `create_schedule(task_id, cron, timezone, payload, deduplication_key, external_id)` / `list_schedules()` / `update_schedule(id, ...)` / `delete_schedule(id)` | `POST/GET /v1/schedules`, `PATCH/DELETE /v1/schedules/{id}` | `deduplication_key` upserts |
| `complete_waitpoint(token, output)` / `get_waitpoint(token)` | `POST /v1/waitpoints/{token}/complete`, `GET /v1/waitpoints/{token}` | Get returns `{"completed": bool, "output": ...}` |
| `lease(worker_id, tasks)` | `POST /v1/workers/{id}/lease` | One due run + its steps, or `None` |
| `report_step(run_id, key, result)` | `POST /v1/workers/leases/{run_id}/step` | Insert-once; duplicate returns recorded result |
| `report_wait(run_id, key, kind, wake_at, token)` | `POST /v1/workers/leases/{run_id}/wait` | `kind`: `"timer"`, `"approval"`, `"child"` |
| `report_complete(run_id, output)` / `report_fail(run_id, worker_id, error)` / `lease_heartbeat(run_id, worker_id)` | `POST /v1/workers/leases/{run_id}/complete`, `/fail`, `/heartbeat` | |

### `TriggerOptions`

`payload`, `delay_ms`, `ttl_ms`, `idempotency_key`,
`idempotency_scope` (`"run"` | `"attempt"` | `"global"`),
`idempotency_ttl_ms`, `concurrency_key`, `max_attempts`, `tags`,
`metadata`. Serialized snake_case, `None` fields omitted — exactly
what the server deserializes.

### `TaskDef` / `RetryPolicy`

`TaskDef(id, version_hash, retry, concurrency_total,
concurrency_per_key, queue)`. `RetryPolicy` defaults match the
server: 5 attempts, 500 ms initial, factor 2.0, 8 s cap, no jitter.
It serializes Rust `Duration` as `{"secs": N, "nanos": N}`.

### `Worker(client, worker_id=None, heartbeat_seconds=10.0)`

- `@worker.task(task_id, version_hash="v1", **def_kwargs)` decorator,
  or `register_task(task_id, fn, version_hash, ...)`.
- `register()` sends definitions to the server.
- `run_once()` leases one run and executes it to completion, failure
  (server retries per policy), or suspension. Returns the run id or
  `None` when no work was available.
- `drive(poll_interval=0.5, max_iterations=None)` loops `run_once`,
  swallowing per-iteration errors after a poll delay (one bad lease
  must not kill the worker).

### `RemoteCtx(client, run_id, steps, attempt)`

- `step(key, fn)` — journaled. Recorded results replay without
  calling `fn`. The server's insert-once rule means a racing
  duplicate report's recorded value wins.
- `sleep(key, seconds)` / `wait_until(key, when)` — timer waits.
  A timer that already fired is a no-op on re-execution (otherwise
  the run would re-suspend forever).
- `wait_for_token(key, token)` — approval wait; returns the token's
  output immediately if already completed.
- `trigger_and_wait(key, task_id, payload)` — child run + suspend;
  re-execution reuses the recorded child id (no duplicate child) and
  returns its output on `COMPLETED`, or raises `RunFailed` on
  `FAILED`/`CANCELLED`/`EXPIRED`.

All wait methods raise `Suspended` (internal control flow, not a
failure) after reporting the wait. `Worker` catches it; task code
should never catch it as an error.

### Errors

`UltraflowsError` base; `ApiError(status, body)`,
`TransportError`, `RunFailed`, `Suspended`.

## How durability works from the Python side

1. **The server never runs task code.** It holds the run, its
   journal (steps), its event log, and its wait state in SQLite
   (Postgres backend in progress, separately).
2. **Steps are the unit of exactly-once.** A task function re-runs
   from the top on every lease (after a wait, retry, or crash), but
   `ctx.step` short-circuits recorded keys. So: put every
   side-effecting or expensive call (LLM, payment, email, HTTP POST)
   inside a `step`. Pure computation between steps may re-run freely.
3. **Waits cost no worker slot.** Reporting a wait moves the run to
   `WAITING`; the worker's lease ends. Timers re-lease when due,
   tokens when completed, children when they complete.
4. **Leases are 30 s, heartbeats every 10 s.** If a worker dies, its
   lease expires and another worker reclaims the run. Attempts are
   therefore at-least-once; recorded steps are exactly-once; external
   effects are exactly-once only if you propagate idempotency keys
   into the external call (the step journal alone cannot undo a
   payment already sent).
5. **Unregistered tasks are visible, never executed.** Triggering a
   task no worker has registered creates a `PENDING_VERSION` run,
   promoted to `PENDING` when the task registers.

## Tests and live proof

```bash
uv run pytest            # stub-transport unit tests, no server needed
python scripts/integration_demo.py   # builds/starts the real Rust
                                     # server, runs a Python worker:
                                     # registration, trigger, step,
                                     # timer wait, completion
```

## Honest limits vs the Rust SDK

- **Synchronous only.** The Rust SDK is async (Tokio); this SDK is
  sync. FastAPI endpoints should use `asyncio.to_thread` (see the
  example). There is no `AsyncClient` yet.
- **No streaming / no batch waitpoints / no webhooks.** Those do not
  exist on the server in this release either; the SDK mirrors the
  server, nothing more.
- **Task functions are sync callables.** An `async def` task function
  is not awaited — use sync code inside tasks, or run your own event
  loop inside the function.
- **No auth.** The server has none in this release; the client sends
  none. Localhost-only deployment is the security boundary.
- **Retry-policy wire shape is Rust-flavoured** (`Duration` as
  `secs`/`nanos`); that is what the server deserializes today.
