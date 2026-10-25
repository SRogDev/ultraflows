# Trigger-core spec delta — what changed vs v0.1.0

v0.1.0 was a single-node demo: the server ran three hardcoded workflows
in-process (`client-onboarding`, `flaky-report`, `delayed-reminder`),
and the API was `POST /v1/runs` with a workflow name. This increment
turns UltraFlows into a functional clone of Trigger.dev's **core
runtime only** (see `docs/TRIGGER_CORE_MAP.md` for the REA-based
analysis it implements). The durability engine did not change; the
surface around it did.

## What stays (unchanged engine)

- Journaled step memoization + deterministic replay over an
  append-only event history (ADR-0001). A recorded step never
  re-executes for that run; attempts are at-least-once; an external
  effect is exactly-once only if the external system accepts the
  propagated idempotency key.
- The `Store` trait + SQLite backend (ADR-0003). Postgres is still
  trait-shaped only, not implemented.
- The embedded library `Executor` + `Ctx` (crates `ultraflows`,
  `ultraflows-core`) for in-process use and tests. It now also
  promotes `PendingVersion` runs once their task registers, and honors
  per-task retry policies when given.
- Waits as persisted data, not sleeping processes.

## What changed

### 1. Task registry replaces hardcoded workflows

- External apps/workers register task definitions via
  `POST /v1/workers/register`: `{ id, version_hash, retry,
  concurrency_total, concurrency_per_key, queue }`.
- Runs reference a registered task id, not a server-side workflow
  function. The server's hardcoded workflow module is deleted; the
  server never runs task code itself.
- A run triggered for an unregistered task is created in
  `PendingVersion`: visible via `GET /v1/runs`, never executed, and
  promoted to `Pending` when the task registers (also by the embedded
  executor's promotion pass). It is not a 400 and not a silent
  execution against wrong code.

### 2. Trigger API (own protocol, NOT wire-compatible with Trigger.dev)

- `POST /v1/tasks/{id}/trigger` with options: `delay_ms`, `ttl_ms`,
  `idempotency_key` (+ `idempotency_scope`: `run`/`attempt`/`global`,
  `idempotency_ttl_ms`), `concurrency_key`, `max_attempts` override,
  `tags`, `metadata`.
- `POST /v1/tasks/{id}/batch-trigger`, capped at 100 items (an
  UltraFlows cap; Trigger's own docs disagree between 100 and 1000,
  per the prior recon — we claim neither of Trigger's numbers).
- `GET /v1/runs` with `status` / `task` / `tag` / `from` / `to`
  filters; `GET /v1/runs/{id}`, `/events`, `/steps`, `/waits/{key}`.
- `POST /v1/runs/{id}/cancel` (same semantics as v0.1.0),
  `/replay` (terminal source only; new run linked by `source_run_id`),
  `/reschedule` (delayed, not-yet-started runs only: `Pending` with
  attempt 0).
- `POST /v1/schedules`, `PATCH /v1/schedules/{id}`,
  `DELETE /v1/schedules/{id}`, `GET /v1/schedules`.
- `POST /v1/waitpoints/{token}/complete` (alias kept:
  `POST /v1/approvals/{token}`) and `GET /v1/waitpoints/{token}`.
- Removed: `POST /v1/runs` (workflow-name trigger). The old
  `scripts/demo.sh` now forwards to `scripts/trigger-demo.sh`.

### 3. Worker protocol + Rust SDK (new crate `ultraflows-sdk`)

- Lease protocol over HTTP: register → lease (journal snapshot
  included) → report steps / waits / completion / failure → heartbeat.
  Lease 30 s (Trigger's visibility timeout per the REA map),
  heartbeat every 10 s from the SDK worker.
- `RemoteCtx` mirrors `Ctx`: `step` (journaled, insert-once),
  `sleep` / `wait_until` (timer), `wait_for_token` (approval/external),
  `trigger_and_wait` (child run + waitpoint; the parent wakes when
  the child completes and reuses the recorded child id on re-execution,
  never triggering a duplicate child).
- Concurrency caps (total + per-key) enforced at claim time in the
  lease handler; blocked runs are skipped rather than head-of-line
  blocking the queue. Invariant-tested at the HTTP layer
  (`crates/ultraflows-server/tests/api_smoke.rs`).
- Retries on the remote path use the task's registered policy
  (factor/min/max, optional deterministic jitter via
  `delay_with_jitter`). The embedded executor uses the same policy
  shape without jitter unless the policy says otherwise.

### 4. Schedules

- Cron with IANA timezone, DST-tested (spring-forward and fall-back
  for `America/Havana`, plus every-minute), via the `cron` crate.
  Trigger's `L` (last-day-of-month) syntax is **rejected with a clear
  400**, not silently misfired — the `cron` crate does not support it.
  (The `croner` crate was evaluated and rejected: it mishandled the
  spring-forward DST gap in probes.)
- Imperative schedules with `deduplication_key` upsert (same key →
  same schedule, updated in place).
- The server's scheduler loop (1 s tick) fires due schedules and
  expires TTLs. It replaces the server's in-process drive loop.
- Declarative (code-defined) schedules are NOT in this increment.

### 5. TTL, delay, expiry

- `Pending`, never-started runs past `ttl_at` become `Expired`
  (terminal) on the scheduler tick. TTL is set at trigger time and is
  not re-added on re-enqueue (matches the REA map's Trigger finding).
- `delay_ms` parks a run as `Pending` with `next_attempt_at` in the
  future; it is not claimable before then. Reschedule only moves
  delayed, not-yet-started runs.

### 6. Data-model additions

- `Run`: `concurrency_key`, `tags`, `metadata`, `parent_run_id`,
  `parent_wait_key`, `schedule_id`, `ttl_at`, `source_run_id`,
  `leased_by`, `lease_expires_at`; statuses `PendingVersion`
  (unregistered task) and `Expired` (TTL).
- New tables: `tasks`, `schedules`, `idempotency_keys`, `workers`.
  Existing v0.1.0 databases are migrated in place (missing `runs`
  columns are added on `migrate()`).
- Idempotency: one SQLite transaction (BEGIN IMMEDIATE on the
  single-connection pool) checks the key and creates the run
  atomically. Scopes: `run` = `task:key`, `global` = `key` alone,
  `attempt` treated like `run` for top-level triggers (no attempt
  exists yet at trigger time). Default key TTL is 30 days — an
  UltraFlows choice from the docs recon; the REA map (§9.3) notes
  Trigger's code paths read did not establish a default, so this
  number is not attributed to Trigger.

## Deliberate differences from Trigger.dev (not bugs, documented)

- **Own protocol, not wire-compatible.** No Trigger.dev hosted-API
  compatibility is claimed; the deploy/build system, versioning, and
  realtime-token machinery are out of scope.
- **DB queue, not Redis.** Leases are DB rows with expiry, not Redis
  visibility timeouts; no fair-queue schedulers (DRR/round-robin/
  weighted), no burst factor, no shared named limits, no gates.
- **Retry defaults differ.** UltraFlows default: max 5, factor 2,
  500 ms–8 s, no jitter. Trigger's defaults per the REA map: max 3,
  factor 2, 1000 ms–60 s, randomized. Apps set the policy explicitly
  at registration; jitter here is deterministic
  (`delay_with_jitter`, `[0.5×delay, delay]`) vs Trigger's
  `[1×, 2×)` randomized form.
- **All waits are durable.** Trigger executes waits ≤ 5000 ms inline
  in the worker; UltraFlows always persists a waitpoint (simpler,
  more durable, slightly more round-trips).
- **No batch waitpoint.** `batch-trigger` creates independent runs;
  there is no `BatchTaskRun`, no `batchTriggerAndWait`, no
  `PARTIAL_FAILED` tracking.
- **No debounce, no gates, no webhooks, no checkpoints (CRIU).**
  See ADR-0009 for the full exclusion list.
- **Single node, SQLite only, no auth.** The server binds
  localhost-only. Concurrent duplicate triggers are serialized by the
  single-connection pool; multi-worker safety beyond lease expiry is
  not claimed.
