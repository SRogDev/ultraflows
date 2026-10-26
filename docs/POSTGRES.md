# PostgreSQL backend for UltraFlows

PostgreSQL is the real backend (ADR-0003, decision C). SQLite stays
the zero-infra default; the server picks PostgreSQL whenever the
database URL starts with `postgres://` or `postgresql://`.
Everything else in this document was verified against a live local
PostgreSQL — a Postgres backend that was never run against Postgres
is not "done", and this one was.

## Install and start (on this VM, verified 2026-10-26)

```sh
# Ubuntu 24.04, as root. NOTE: `apt-get update` hung on one source
# behind this VM's egress proxy (network itself was fine); killing it
# and installing from the already-present package lists worked.
# If `dpkg` reports an interrupted run, `dpkg --configure -a` first.
DEBIAN_FRONTEND=noninteractive apt-get install -y postgresql
# Installed: PostgreSQL 16.15 (Ubuntu 16.15-0ubuntu0.24.04.1),
# cluster 16/main on 127.0.0.1:5432. The container's policy-rc.d
# blocks the automatic service start, so start the cluster by hand:
pg_ctlcluster 16 main start
pg_lsclusters   # 16 main 5432 online
```

On a normal machine (systemd, no policy-rc.d block), `apt-get
install postgresql` starts the cluster itself and the `pg_ctlcluster`
step is unnecessary. The data directory is
`/var/lib/postgresql/16/main`.

## Roles and databases (local development only)

The commands below use a dedicated role with a weak, local-only
password. Do not reuse this password anywhere real; in production the
connection string should come from a secret store, and the role
should own only its own database (it needs `CREATEDB` here only so
the demo/test scripts can recreate their databases).

```sh
su postgres -c "psql -v ON_ERROR_STOP=1" <<'SQL'
DO $$ BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'ultraflows') THEN
    CREATE ROLE ultraflows LOGIN PASSWORD 'ultraflows' CREATEDB;
  END IF;
END $$;
CREATE DATABASE ultraflows_test OWNER ultraflows;
CREATE DATABASE ultraflows_demo OWNER ultraflows;
SQL
# Check TCP auth works (host connections use scram-sha-256 by default):
PGPASSWORD=ultraflows psql -h 127.0.0.1 -U ultraflows -d ultraflows_test \
  -c 'SELECT current_user;'
```

## Running the server on PostgreSQL

```sh
ULTRAFLOW_DATABASE_URL=postgres://ultraflows:ultraflows@127.0.0.1:5432/ultraflows_demo \
  PORT=3000 cargo run --bin ultraflows-server
```

Backend selection, in order:

1. `ULTRAFLOW_DATABASE_URL` starting with `postgres://` or
   `postgresql://` → PostgreSQL.
2. Otherwise `ULTRAFLOW_DB` starting with `postgres://` or
   `postgresql://` → PostgreSQL (same effect, kept so a single env
   var can switch backends).
3. Anything else (including the default `ultraflows.db`) → SQLite,
   exactly as in v0.1.0. Existing SQLite scripts keep passing.

The server binds 127.0.0.1 only and has no auth in this release —
same as the SQLite server. Do not expose it to a network you do not
trust, and do not put a production Postgres URL on a shared host
without TLS + auth in front of it (not implemented here).

## Schema: how it is created

`PostgresStore::migrate()` (called by the server at startup, and by
the tests) runs the schema in
`crates/ultraflows-store/src/postgres.rs` (`PG_SCHEMA`) with
`CREATE TABLE IF NOT EXISTS` / `CREATE INDEX IF NOT EXISTS`, so
starting the server twice — or restarting after a `kill -9` — is
safe and reuses the data. There is deliberately no ALTER-based
upgrade path for PostgreSQL: no PostgreSQL schema existed before
this change, so there is nothing to migrate from. (SQLite keeps its
own `ALTER TABLE ... ADD COLUMN` upgrade path for v0.1.0 files.)

Tables: `runs`, `tasks`, `schedules`, `idempotency_keys`, `workers`,
`steps`, `events`, `event_counters`, `waits`, `approvals` — the same
set as SQLite, plus `event_counters` (see below).

## Differences vs SQLite

| Area | SQLite | PostgreSQL |
|---|---|---|
| Ids | `TEXT` UUID strings | Native `UUID` |
| Payloads / outputs / tags / metadata / retry policy | `TEXT` JSON | `JSONB`, same JSON shape (verified: API responses are identical) |
| Timestamps | `INTEGER` epoch ms | `BIGINT` epoch ms (same values, same comparisons) |
| Flags (`cancel_requested`, `enabled`) | `INTEGER` 0/1 | `BOOLEAN` |
| Pool | 1 connection (single-writer store) | 5 connections (untuned default, see below) |
| Step insert-once | `INSERT OR IGNORE` | `INSERT ... ON CONFLICT DO NOTHING`, rows-affected check |
| Event sequences | `MAX(seq)+1` in a transaction (safe only on the single-connection pool) | `event_counters` row per run, atomically incremented with `INSERT ... ON CONFLICT DO UPDATE ... RETURNING last_seq`; a rolled-back transaction rolls its increment back, so sequences stay gapless |
| Claims (`claim_due`, `claim_specific`) | Transaction on the single connection | `SELECT ... FOR UPDATE SKIP LOCKED` in a transaction: two processes racing on the same run cannot both claim it |
| Idempotent trigger | `BEGIN IMMEDIATE` serializes writers (single-node only) | Transaction-scoped advisory lock per storage key (`pg_advisory_xact_lock(hashtext(...))`): same-key triggers serialize, different keys proceed in parallel |
| Schedule dedup | Partial unique index | Same partial unique index (`WHERE deduplication_key IS NOT NULL`) |

One sqlx/Postgres gotcha worth knowing: sqlx's `Json` wrapper binds
as `json`, and PostgreSQL refuses to store `json` into a `jsonb`
column implicitly. Every JSONB placeholder in the Postgres code is
therefore written as `$n::jsonb`. If you add a JSONB column, add the
cast or the insert fails with error 42804.

## Tests

Postgres store tests live in `crates/ultraflows-store/src/postgres.rs`
(3 basic: step insert-once, gapless events, claim-due) and
`postgres_ext.rs` (1 conformance test covering run lifecycle,
idempotent trigger incl. global scope, lease claim/expiry, running
counts, cancel, TTL expiry, child wake, task registry, schedule
dedup, waits, approval, worker heartbeat, PendingVersion promotion,
list_due). They run against a LIVE Postgres:

```sh
# Fresh dedicated test database first (drop + create):
su postgres -c "psql -c 'DROP DATABASE ultraflows_test;' -c 'CREATE DATABASE ultraflows_test OWNER ultraflows;'"
ULTRAFLOW_TEST_DATABASE_URL=postgres://ultraflows:ultraflows@127.0.0.1:5432/ultraflows_test \
  cargo test -p ultraflows-store pg_
# DATABASE_URL is accepted as a fallback if ULTRAFLOW_TEST_DATABASE_URL is unset.
```

Two behaviours are deliberate, not accidents:

- **Skip, don't fail, without a URL.** If neither env var is set,
  each Postgres test returns early and passes, so `cargo test
  --workspace` stays green on machines without Postgres.
- **No truncation, fresh DB instead.** The store tests run in
  parallel in one test binary and share the test database, so each
  test asserts only on runs/schedules it created itself. Truncating
  inside a test would delete a sibling test's data mid-flight. Point
  the tests at a fresh database (drop/create above) instead.

## Demo on PostgreSQL

```sh
scripts/trigger-demo-pg.sh   # 15/15 on live Postgres 16.15
```

It drops and recreates `ultraflows_demo` (never point it at a
database you care about), boots the server on PostgreSQL, runs the
external demo worker, and proves: external registration, trigger,
idempotent re-trigger, retry to attempt 3, timer wait,
triggerAndWait with the child's receipt, and approval wait +
`kill -9` + restart + resume with `prepare` recorded exactly once.
The SQLite demo (`scripts/trigger-demo.sh`, 18/18) additionally
covers schedule firing and batch trigger, which share the same
trigger/lease code paths and are not re-proven here to keep this
script fast. `PG_URL`/`PG_MAINT_URL` env vars override the demo and
maintenance connection strings (defaults in the script header).

## What is NOT covered (honest list)

- **Connection pool tuning.** 5 connections is an untuned default.
  No load testing was done; tune `max_connections` (in
  `PostgresStore::connect`) with real traffic before production use.
- **Multi-node safety.** Claims and idempotent triggers are safe
  across processes (row locks / advisory locks above), but the
  scheduler loop is not leader-elected: two servers sharing one
  database can both fire the same schedule tick. Run one scheduler
  (or one server) per database until leader election exists.
- **Auth / TLS / secrets.** No server auth (localhost-only bind),
  no TLS on the Postgres connection in this configuration, and the
  example password above is for local development only.
- **Migrations framework.** Schema creation is `IF NOT EXISTS`
  only; there is no versioned migration history or rollback. A
  future schema change needs a real migration step, not another
  `CREATE TABLE IF NOT EXISTS`.
- **Performance numbers.** None measured; no claim is made about
  throughput or latency on either backend.
