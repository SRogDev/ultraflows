# STATUS — UltraFlows

## Done

- 2026-10-23 — Phase 0 scaffold: workspace (edition 2024, Rust 1.98.1),
  4 crates (core / store / runtime / server), deliberate dependency set
  downloaded and locked, base docs (README, ROADMAP, ARCHITECTURE,
  TESTING, SECURITY_RELIABILITY), ADRs 0001–0007.
- 2026-10-23 — Foundational decisions (Roger): D1 durability =
  Temporal-style journaled replay; D2 = service-first; D3 = Store trait
  (Postgres real, SQLite/memory for tests); D4 = code-first Rust
  definitions; unspecified behavior defaults to Temporal (ADR-0007).
- 2026-10-23 — Phase 0 verified: `cargo build`, `cargo test` (0 tests —
  scaffold, honestly), `cargo clippy -D warnings` and `cargo fmt
  --check` all green; `ultraflows-server` binary runs and logs its
  scaffold status. Stack resolved in Cargo.lock: tokio 1.54.1,
  sqlx 0.8.6, serde 1.0.229, chrono 0.4.45, uuid 1.26.1, proptest 1.11.0.

## Done (Trigger-core, 2026-10-25)

- Functional clone of Trigger.dev's CORE RUNTIME in Rust (runtime
  only — no UI, deploy, billing, AI, checkpoints, ClickHouse/Redis).
  REA map (`docs/TRIGGER_CORE_MAP.md`, 7 evidence files) + spec delta
  + ADR-0009. Task registry, Trigger-shaped API, remote-worker lease
  protocol, `ultraflows-sdk` (RemoteCtx + Worker), external demo app.
  **25 tests pass** (core 9, store 9, runtime 4, server 3, SDK 0 +
  demo script 18/18 proofs). `scripts/trigger-demo.sh` is canonical;
  `scripts/demo.sh` forwards to it. v0.1.0 in-process workflows and
  `POST /v1/runs` removed. Commits on local main only (no push).

## Done (Postgres + Python SDK, 2026-10-25, ADR-0010)

- **Postgres real** (not just the trait): `PostgresStore` with the
  same invariants as SQLite (insert-once steps, gapless events via
  `event_counters`, `SELECT ... FOR UPDATE SKIP LOCKED` claims,
  advisory-lock idempotent triggers, JSONB payloads, same JSON
  shape). Backend chosen by `ULTRAFLOW_DATABASE_URL` /
  `postgres://` `ULTRAFLOW_DB`; SQLite stays the default. Tested
  against live PostgreSQL 16.15 installed on this VM; store tests
  create one fresh database per test (`uf_test_<uuid>`) after an
  earlier shared-DB version failed on `claim_due` races.
  `scripts/trigger-demo-pg.sh` 15/15 on live Postgres.
- **SDK Python** (`sdk-python/`, uv project, package `ultraflows`):
  synchronous `Client` / `Worker` / `RemoteCtx` mirroring the Rust
  SDK protocol exactly (same endpoints, snake_case JSON,
  `SCREAMING_SNAKE_CASE` statuses), `httpx` with `urllib` fallback,
  23 pytest tests + ruff clean, live integration demo against the
  real Rust server ALL PASS, and a FastAPI + LangGraph example
  (endpoint triggers and returns run id; LangGraph graph inside a
  worker task with nodes wrapped in `ctx.step()`). Sync only — no
  `AsyncClient` yet (FastAPI uses `asyncio.to_thread`).
- Workspace totals after both increments: **29 Rust tests** (core 9,
  store 13, runtime 4, server smoke 3) + **23 Python tests**;
  fmt + clippy `-D warnings` green; SQLite demo 18/18, Postgres
  demo 15/15, Python integration demo ALL PASS. Commits on local
  main only (no push).

## Doing

- Consolidated docs + interview PDF for the full increment.

## Next

- Phase 1A implementation (only on Roger's go): minimal durable slice
  against in-memory/SQLite store, fault-injection harness.
- Phase 1B: server + Postgres + minimal worker protocol.
- First publication (2026-10-24): public on GitHub as SRogDev/ultraflows
  under the Elastic License 2.0 — ADR-0006 decided (license + name).

## Done (v0.1.0, 2026-10-25)

- Functional single-node runtime: core state machine + retry policy,
  SQLite Store, journaled-step replay, timer + approval waits, retries,
  cancellation, event log, axum HTTP API, in-process worker, 3 demo
  workflows. **11/11 tests pass**; fmt + clippy -D warnings green.
- Demo: `scripts/demo.sh` — approval wait, `kill -9` + restart,
  resume without re-executing steps, flaky retries, timer wake.
- ADR-0008 records the portfolio scope and its limits honestly
  (Postgres, remote workers, lease expiry, auth, schedules: not yet).
