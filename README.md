# UltraFlows (temporary name)

A native Rust **durable workflow runtime**: long-running workflows — many
steps, dependencies, waits, retries, schedules, external tool calls and
human approvals — that survive process crashes and restarts, over days,
months or years.

UltraFlows is being built as the future durable-execution layer for
Polygrow, but it is **not coupled to Polygrow**: it knows nothing about
business logic, tenants' companies, agent frameworks or AI models. An AI
call, for the runtime, is just one more external step whose *recorded
result* is durable — the runtime never assumes it is deterministic.

## Status

**Trigger-core runtime — functional single-node clone of Trigger.dev's
core runtime in Rust (2026-10-25).** External apps register tasks,
trigger runs over HTTP, and work them as remote workers via the
`ultraflows-sdk` crate: journaled steps with replay, timer /
approval-token / child-run waits (`triggerAndWait`), retries from the
task's policy, concurrency caps at claim time, cron schedules with
IANA timezones, idempotent triggers, TTL + delay + reschedule, and an
append-only event log — on SQLite (default) or real PostgreSQL 16
(`ULTRAFLOW_DATABASE_URL`), with 29 tests, an 18-proof end-to-end
demo (`scripts/trigger-demo.sh`, including `kill -9` + restart +
resume without re-executing steps) and a 15-proof Postgres demo
(`scripts/trigger-demo-pg.sh`). Python apps call it through
`sdk-python/` (Client + Worker + RemoteCtx, incl. a FastAPI +
LangGraph example). See "Limitations" below and
`docs/adr/0009-trigger-core-scope.md` (plus ADR-0010 for Postgres
and the Python SDK) for exactly what is *not* implemented yet.
Nothing here is claimed durable, reliable or fast beyond what the
tests and the demos prove. This is Trigger-*shaped*, not
wire-compatible with Trigger.dev's hosted API.

## For recruiters (30 seconds)

UltraFlows is a Trigger-shaped durable workflow runtime in Rust:
task code runs in remote workers, steps are journaled, and a run's
event history lets it survive crashes and long waits. The demo proves
the core claim end-to-end: an external app registers tasks, triggers
runs, one run waits on a human approval, the server is killed with
`kill -9`, restarted, the approval completes, and the run finishes —
with earlier steps replayed from the journal, never re-executed.

## Quickstart

```bash
# Run the full demo: external registration, trigger, approval wait +
# kill -9 + restart + resume, retry, timer, triggerAndWait, schedule,
# idempotent re-trigger, batch trigger (18 proofs)
bash scripts/trigger-demo.sh

# Or run the server yourself (localhost-only, no auth)
ULTRAFLOW_DB=ultraflows.db PORT=3000 cargo run --bin ultraflows-server
# On Postgres instead (see docs/POSTGRES.md for setup):
ULTRAFLOW_DATABASE_URL=postgres://ultraflows:ultraflows@127.0.0.1:5432/ultraflows_demo PORT=3000 cargo run --bin ultraflows-server
# Python worker (see sdk-python/README.md):
cd sdk-python && uv run python scripts/integration_demo.py
curl -s localhost:3000/health
# In another terminal: register + work demo tasks, then trigger one
ULTRAFLOWS_URL=http://127.0.0.1:3000 cargo run --bin ultraflows-demo-app -- worker &
curl -s -X POST localhost:3000/v1/tasks/tick-task/trigger \
  -H 'content-type: application/json' -d '{"payload":{}}'
```

Demo tasks (in `ultraflows-demo-app`, not in the server): `onboarding`
(parent + child), `charge-card`, `approval-flow`, `flaky-report`,
`timer-task`, `tick-task`. API: `POST /v1/tasks/{id}/trigger`,
`/batch-trigger`, `GET /v1/runs[?status&task&tag&from&to]`,
`GET /v1/runs/{id}[/events|/steps|/waits/{key}]`,
`POST /v1/runs/{id}/cancel|/replay|/reschedule`,
`POST /v1/schedules`, `POST /v1/waitpoints/{token}/complete`,
worker protocol under `/v1/workers/...`. Full surface:
`docs/TRIGGER_CORE_SPEC_DELTA.md`.

## Limitations (stated plainly)

- Single node; SQLite (default) or PostgreSQL via
  `ULTRAFLOW_DATABASE_URL` (pool 5, untuned; two servers sharing one
  database can double-fire a schedule — no leader election yet).
  Own protocol — NOT wire-compatible with Trigger.dev's hosted API.
- No authentication — bind is localhost-only; do not expose it to
  an untrusted network.
- No declarative (code-defined) schedules, no batch waitpoints, no
  debounce, no gates, no webhooks, no checkpoints, no multi-tenancy.
- Cron `L` syntax is rejected with a clear error (unsupported).
- A never-completed token wait blocks forever unless cancelled
  (waitpoint timeout semantics unestablished).
- Remote `Running` runs are reclaimed via 30 s lease expiry; the
  embedded executor has no lease expiry.
- No performance claims: no benchmark has been run yet.

## Concepts (kept distinct on purpose)

| Concept | Meaning |
|---|---|
| Workflow definition | The code/shape of a process, versioned |
| Workflow execution (run) | One execution of a definition, with its own durable state |
| Step | One named unit of work inside a run, whose result is journaled |
| Attempt | One try at executing a run; a run may need several |
| Business task | A Polygrow-level concept — *not* every business task becomes a durable workflow |

## Honesty contract

- Attempts execute **at-least-once**.
- A step whose result was recorded is never executed again for that run.
- An external effect is exactly-once **only** if the external system
  accepts the idempotency key UltraFlows propagates. No universal
  exactly-once is claimed, ever.

## Layout

```text
crates/
  ultraflows-core/    pure types, state machine, invariants (no Tokio, no I/O)
  ultraflows-store/   Store trait + SQLite and PostgreSQL backends
  ultraflows/         runtime library: embedded Ctx/Executor (kept for library use)
  ultraflows-sdk/     Rust SDK: Client (trigger) + Worker + RemoteCtx (remote tasks)
  sdk-python/         Python SDK (same protocol) + FastAPI/LangGraph example
  ultraflows-demo-app/ external demo app (registers + triggers + works tasks)
  ultraflows-server/  the service binary (Trigger-shaped API + scheduler loop)
docs/adr/             architecture decision records
docs/TRIGGER_CORE_MAP.md, docs/TRIGGER_CORE_SPEC_DELTA.md
```

Runnable demos live in `scripts/trigger-demo.sh` (`scripts/demo.sh`
forwards to it). A benchmark harness arrives when there is something
real to measure — no numbers are quoted before then.

## Development

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Toolchain is pinned in `rust-toolchain.toml` (Rust 1.98.1, edition 2024).
See `ROADMAP.md` for the four phases and their exit criteria,
`ARCHITECTURE.md` for boundaries and invariants, and `TESTING.md` for the
testing and benchmarking strategy.

## License

Elastic License 2.0 — see `LICENSE`. Decided 2026-10-24 in
`docs/adr/0006-license-and-name.md`: source-available, so the runtime
stays public and studyable while nobody may resell it as a hosted
service.
