# ADR-0010 — Real Postgres backend + Python SDK

- **Status:** Accepted (2026-10-25)
- **Context:** After the Trigger-core increment (ADR-0009), two gaps
  blocked real use: the only backend was SQLite (Postgres existed
  only as a `Store` trait shape), and the only SDK was Rust — while
  Roger's services are FastAPI + LangGraph in Python. Roger ordered
  both: real Postgres ("es lo que uso") and a Python SDK.
- **Decision:**
  1. **PostgresStore implements the same `Store` trait** as
     `SqliteStore` (no trait change): tasks, schedules, idempotency
     keys, leases, TTL, child wake, heartbeats, runs/steps/events/
     waits. Postgres types: native `UUID`, `JSONB` (same JSON shape
     as SQLite's TEXT JSON — note the sqlx trap: `Json` binds as
     `json`, so every JSONB placeholder needs an explicit
     `$n::jsonb` cast), `BIGINT` epoch-ms timestamps, `BOOLEAN`
     flags. Schema is created idempotently by `migrate()`
     (`CREATE TABLE IF NOT EXISTS`); there is no migrations
     framework yet, so a future schema change needs a real
     migration step.
  2. **Backend selection without object safety:** the trait's
     `async fn` is not object-safe, so `Arc<dyn Store>` was not an
     option without restructuring the server. Instead an `AnyStore`
     enum (SQLite | Postgres) delegates the trait. Selection:
     `ULTRAFLOW_DATABASE_URL`, or `ULTRAFLOW_DB` starting with
     `postgres://`/`postgresql://` → Postgres; anything else →
     SQLite. **Default stays SQLite.**
  3. **Concurrency where Postgres differs from single-writer
     SQLite:** claims use `SELECT ... FOR UPDATE SKIP LOCKED`;
     idempotent triggers serialize per key with a transaction
     advisory lock (`pg_advisory_xact_lock(hashtext(...))`);
     gapless event sequences come from an atomically updated
     `event_counters` row (rollback-safe).
  4. **Postgres tests are isolated per test database.** The first
     version shared one test database across parallel tests and
     failed: `claim_due` is global within a database, so it claimed
     a sibling test's run, and leftover rows broke repeat runs.
     `pg_store()` now creates `uf_test_<uuid>` per test and
     migrates it; the databases are left behind and cleaned up
     with the query in `docs/POSTGRES.md`. Without a test URL the
     Postgres tests skip (pass) instead of failing.
  5. **Python SDK mirrors the Rust SDK protocol exactly** — same
     endpoints, same snake_case JSON, same `SCREAMING_SNAKE_CASE`
     statuses (the Rust demo once broke on `"Completed"` vs
     `"COMPLETED"`; the Python SDK compares only the screaming
     form). It is synchronous only (no `AsyncClient` yet; FastAPI
     endpoints use `asyncio.to_thread`) and lives in
     `sdk-python/` as a uv project (`httpx`, with a stdlib
     `urllib` fallback if httpx is absent). Its example shows
     both patterns Roger uses: a FastAPI endpoint that triggers
     and returns the run id, and a LangGraph graph inside a worker
     task with nodes wrapped in `ctx.step()`.
- **Consequences / honest limits:**
  - Pool size 5 is an untuned default; no load testing, no
    performance claims.
  - Multi-node safety is partial: claims and idempotent triggers
    are safe across processes, but the scheduler loop is not
    leader-elected — two servers sharing one database can
    double-fire a schedule. Run one server per database until
    leader election exists.
  - No server auth (localhost-only bind), no TLS on the Postgres
    connection in the documented setup, and the documented dev
    password is local-dev only.
  - The Python SDK does not yet cover batch waitpoints, webhooks,
    or auth, because the server does not have them either.
- **Evidence (2026-10-25, verified by the principal, not only by
  the implementing agents):** Rust workspace 29/29 (core 9, store
  13, runtime 4, server smoke 3) with and without the Postgres
  test URL; Python 23/23 pytest + ruff clean; SQLite demo 18/18;
  Postgres demo 15/15 on live PostgreSQL 16.15; Python live
  integration demo ALL PASS against the real Rust server.
