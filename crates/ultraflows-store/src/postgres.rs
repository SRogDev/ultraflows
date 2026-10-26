//! PostgreSQL backend for the `Store` trait.
//!
//! Same tables and invariants as the SQLite backend (see `sqlite.rs`),
//! with PostgreSQL types: UUID ids, JSONB payloads (same JSON shape as
//! SQLite's TEXT JSON), BIGINT epoch-millis timestamps, BOOLEAN flags.
//! Schema is created idempotently by `migrate()`.
//!
//! Concurrency notes (where PostgreSQL differs from the single-writer
//! SQLite pool):
//! - `claim_due` / `claim_specific` use `SELECT ... FOR UPDATE SKIP
//!   LOCKED` inside a transaction, so two server processes racing on
//!   the same run cannot both claim it.
//! - `create_run_idempotent` serializes same-key triggers with a
//!   transaction-scoped advisory lock on the storage key.
//! - `append_event` allocates gapless per-run sequences from a
//!   `event_counters` row updated atomically (`ON CONFLICT DO UPDATE
//!   ... RETURNING`), so concurrent appends cannot collide or gap.
//!   (A rolled-back transaction rolls its counter increment back too.)
//!
//! Pool size is a deliberately untuned default (5); see docs/POSTGRES.md.
//! Multi-node safety is NOT claimed: two servers sharing one database
//! are safe for claims/idempotency (above), but scheduler ticks from
//! two servers can still double-fire a schedule (same limitation the
//! SQLite server has with one process; documented, not hidden).

use crate::error::StoreError;
use crate::sqlite::{dt, status};
use chrono::Utc;
use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::types::Json;
use sqlx::{Pool, Postgres, Row};
use ultraflows_core::types::Run;

pub(crate) const PG_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS runs (
    id UUID PRIMARY KEY,
    workflow TEXT NOT NULL,
    payload JSONB NOT NULL,
    output JSONB,
    error TEXT,
    status TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    max_attempts INTEGER NOT NULL,
    next_attempt_at BIGINT,
    wake_at BIGINT,
    wait_key TEXT,
    wait_kind TEXT,
    wait_token TEXT,
    cancel_requested BOOLEAN NOT NULL DEFAULT FALSE,
    concurrency_key TEXT,
    tags JSONB NOT NULL DEFAULT '[]'::jsonb,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    parent_run_id UUID,
    parent_wait_key TEXT,
    schedule_id TEXT,
    ttl_at BIGINT,
    source_run_id UUID,
    leased_by TEXT,
    lease_expires_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_runs_status_created ON runs(status, created_at);
CREATE INDEX IF NOT EXISTS idx_runs_workflow_status ON runs(workflow, status);
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    version_hash TEXT NOT NULL,
    retry_json JSONB NOT NULL,
    concurrency_total INTEGER,
    concurrency_per_key INTEGER,
    queue TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS schedules (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    cron TEXT NOT NULL,
    timezone TEXT NOT NULL,
    external_id TEXT,
    deduplication_key TEXT,
    payload JSONB NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    next_fire_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_schedules_dedup
    ON schedules(deduplication_key) WHERE deduplication_key IS NOT NULL;
CREATE TABLE IF NOT EXISTS idempotency_keys (
    scope TEXT NOT NULL,
    key TEXT NOT NULL,
    run_id UUID NOT NULL,
    expires_at BIGINT NOT NULL,
    PRIMARY KEY (scope, key)
);
CREATE TABLE IF NOT EXISTS workers (
    id TEXT PRIMARY KEY,
    last_seen BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS steps (
    run_id UUID NOT NULL,
    key TEXT NOT NULL,
    result JSONB NOT NULL,
    idempotency_key TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (run_id, key)
);
CREATE TABLE IF NOT EXISTS events (
    id BIGSERIAL PRIMARY KEY,
    run_id UUID NOT NULL,
    seq BIGINT NOT NULL,
    kind TEXT NOT NULL,
    payload JSONB NOT NULL,
    created_at BIGINT NOT NULL,
    UNIQUE (run_id, seq)
);
CREATE TABLE IF NOT EXISTS event_counters (
    run_id UUID PRIMARY KEY,
    last_seq BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS waits (
    run_id UUID NOT NULL,
    key TEXT NOT NULL,
    kind TEXT NOT NULL,
    token TEXT,
    wake_at BIGINT,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (run_id, key)
);
CREATE TABLE IF NOT EXISTS approvals (
    token TEXT PRIMARY KEY,
    result JSONB NOT NULL,
    created_at BIGINT NOT NULL
);
";

pub(crate) fn pg_run_from_row(r: &PgRow) -> Result<Run, StoreError> {
    Ok(Run {
        id: r.try_get("id")?,
        workflow: r.try_get("workflow")?,
        payload: r.try_get::<Json<serde_json::Value>, _>("payload")?.0,
        output: r
            .try_get::<Option<Json<serde_json::Value>>, _>("output")?
            .map(|j| j.0),
        error: r.try_get("error")?,
        status: status(r.try_get("status")?),
        attempt: r.try_get::<i32, _>("attempt")? as u32,
        max_attempts: r.try_get::<i32, _>("max_attempts")? as u32,
        next_attempt_at: dt(r.try_get("next_attempt_at")?),
        wake_at: dt(r.try_get("wake_at")?),
        wait_key: r.try_get("wait_key")?,
        wait_kind: r.try_get("wait_kind")?,
        wait_token: r.try_get("wait_token")?,
        cancel_requested: r.try_get("cancel_requested")?,
        concurrency_key: r.try_get("concurrency_key")?,
        tags: r
            .try_get::<Option<Json<serde_json::Value>>, _>("tags")?
            .and_then(|j| serde_json::from_value(j.0).ok())
            .unwrap_or_default(),
        metadata: r
            .try_get::<Option<Json<serde_json::Value>>, _>("metadata")?
            .map(|j| j.0)
            .unwrap_or(serde_json::json!({})),
        parent_run_id: r.try_get("parent_run_id")?,
        parent_wait_key: r.try_get("parent_wait_key")?,
        schedule_id: r.try_get("schedule_id")?,
        ttl_at: dt(r.try_get("ttl_at")?),
        source_run_id: r.try_get("source_run_id")?,
        leased_by: r.try_get("leased_by")?,
        lease_expires_at: dt(r.try_get("lease_expires_at")?),
        created_at: dt(Some(r.try_get::<i64, _>("created_at")?)).unwrap_or_else(Utc::now),
        updated_at: dt(Some(r.try_get::<i64, _>("updated_at")?)).unwrap_or_else(Utc::now),
    })
}

/// PostgreSQL backend. Pool of 5 connections (untuned default).
#[derive(Debug, Clone)]
pub struct PostgresStore {
    pool: Pool<Postgres>,
}

impl PostgresStore {
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new().max_connections(5).connect(url).await?;
        Ok(Self { pool })
    }

    pub async fn connect_with(
        options: sqlx::postgres::PgConnectOptions,
    ) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &Pool<Postgres> {
        &self.pool
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::traits::Store;
    use ultraflows_core::types::{NewRun, RunStatus, StepRecord};

    /// Live-Postgres store, or None when no test URL is configured.
    /// Env: ULTRAFLOW_TEST_DATABASE_URL, falling back to DATABASE_URL.
    /// Each test gets its OWN freshly created database
    /// (`uf_test_<uuid>`), because `claim_due` and the conformance
    /// counts are global within a database: sharing one DB across
    /// parallel tests (or across runs, via leftover rows) made
    /// `claim_due` claim a sibling test's run instead of its own.
    /// The per-test databases are small and are left behind; drop
    /// them with `DROP DATABASE uf_test_%` style cleanup (see
    /// docs/POSTGRES.md) when they accumulate.
    pub(crate) async fn pg_store() -> Option<PostgresStore> {
        use sqlx::postgres::PgConnectOptions;
        use std::str::FromStr;
        let url = std::env::var("ULTRAFLOW_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .ok()?;
        let base = PgConnectOptions::from_str(&url).ok()?;
        let db = format!("uf_test_{}", uuid::Uuid::now_v7().simple());
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(base.clone().database("postgres"))
            .await
            .ok()?;
        sqlx::query(&format!("CREATE DATABASE \"{db}\""))
            .execute(&admin)
            .await
            .ok()?;
        drop(admin);
        let s = PostgresStore::connect_with(base.database(&db)).await.ok()?;
        s.migrate().await.ok()?;
        Some(s)
    }

    #[tokio::test]
    async fn pg_step_record_is_insert_once_first_result_wins() {
        let Some(s) = pg_store().await else {
            return;
        };
        let run = s
            .create_run(NewRun::new("demo", serde_json::json!({}), 3))
            .await
            .unwrap();
        let rec = StepRecord {
            run_id: run.id,
            key: "charge".to_string(),
            result: serde_json::json!({"receipt": "first"}),
            idempotency_key: ultraflows_core::idempotency_key(run.id, "charge"),
        };
        assert!(s.record_step(&rec).await.unwrap());
        let mut second = rec.clone();
        second.result = serde_json::json!({"receipt": "second"});
        assert!(!s.record_step(&second).await.unwrap());
        let got = s.get_step(run.id, "charge").await.unwrap().unwrap();
        assert_eq!(got.result["receipt"], "first");
    }

    #[tokio::test]
    async fn pg_events_have_gapless_sequence_numbers() {
        let Some(s) = pg_store().await else {
            return;
        };
        let run = s
            .create_run(NewRun::new("demo", serde_json::json!({}), 3))
            .await
            .unwrap();
        for kind in ["Created", "AttemptStarted", "Completed"] {
            s.append_event(run.id, kind, serde_json::json!({}))
                .await
                .unwrap();
        }
        let events = s.list_events(run.id).await.unwrap();
        assert_eq!(
            events.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[tokio::test]
    async fn pg_claim_due_only_claims_due_runs_and_increments_attempt() {
        let Some(s) = pg_store().await else {
            return;
        };
        let run = s
            .create_run(NewRun::new("demo", serde_json::json!({}), 3))
            .await
            .unwrap();
        let claimed = s
            .claim_due(Utc::now().timestamp_millis())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claimed.id, run.id);
        assert_eq!(claimed.attempt, 1);
        assert_eq!(claimed.status, RunStatus::Running);
        assert!(
            s.claim_due(Utc::now().timestamp_millis())
                .await
                .unwrap()
                .is_none()
        );
    }
}
