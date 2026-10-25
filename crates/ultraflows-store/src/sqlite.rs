use crate::error::StoreError;
use chrono::{DateTime, TimeZone, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Row, Sqlite};
use ultraflows_core::types::{Run, RunStatus};
use uuid::Uuid;

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY,
    workflow TEXT NOT NULL,
    payload TEXT NOT NULL,
    output TEXT,
    error TEXT,
    status TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    max_attempts INTEGER NOT NULL,
    next_attempt_at INTEGER,
    wake_at INTEGER,
    wait_key TEXT,
    wait_kind TEXT,
    wait_token TEXT,
    cancel_requested INTEGER NOT NULL DEFAULT 0,
    concurrency_key TEXT,
    tags TEXT NOT NULL DEFAULT '[]',
    metadata TEXT NOT NULL DEFAULT '{}',
    parent_run_id TEXT,
    parent_wait_key TEXT,
    schedule_id TEXT,
    ttl_at INTEGER,
    source_run_id TEXT,
    leased_by TEXT,
    lease_expires_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    version_hash TEXT NOT NULL,
    retry_json TEXT NOT NULL,
    concurrency_total INTEGER,
    concurrency_per_key INTEGER,
    queue TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS schedules (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    cron TEXT NOT NULL,
    timezone TEXT NOT NULL,
    external_id TEXT,
    deduplication_key TEXT,
    payload TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    next_fire_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_schedules_dedup
    ON schedules(deduplication_key) WHERE deduplication_key IS NOT NULL;
CREATE TABLE IF NOT EXISTS idempotency_keys (
    scope TEXT NOT NULL,
    key TEXT NOT NULL,
    run_id TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    PRIMARY KEY (scope, key)
);
CREATE TABLE IF NOT EXISTS workers (
    id TEXT PRIMARY KEY,
    last_seen INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS steps (
    run_id TEXT NOT NULL,
    key TEXT NOT NULL,
    result TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (run_id, key)
);
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (run_id, seq)
);
CREATE TABLE IF NOT EXISTS waits (
    run_id TEXT NOT NULL,
    key TEXT NOT NULL,
    kind TEXT NOT NULL,
    token TEXT,
    wake_at INTEGER,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (run_id, key)
);
CREATE TABLE IF NOT EXISTS approvals (
    token TEXT PRIMARY KEY,
    result TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
";

pub(crate) fn ms(dt: DateTime<Utc>) -> i64 {
    dt.timestamp_millis()
}

pub(crate) fn dt(v: Option<i64>) -> Option<DateTime<Utc>> {
    v.and_then(|m| Utc.timestamp_millis_opt(m).single())
}

pub(crate) fn status(s: &str) -> RunStatus {
    match s {
        "Pending" => RunStatus::Pending,
        "PendingVersion" => RunStatus::PendingVersion,
        "Running" => RunStatus::Running,
        "Waiting" => RunStatus::Waiting,
        "Completed" => RunStatus::Completed,
        "Failed" => RunStatus::Failed,
        "Expired" => RunStatus::Expired,
        _ => RunStatus::Cancelled,
    }
}

pub(crate) fn status_str(s: RunStatus) -> &'static str {
    match s {
        RunStatus::Pending => "Pending",
        RunStatus::PendingVersion => "PendingVersion",
        RunStatus::Running => "Running",
        RunStatus::Waiting => "Waiting",
        RunStatus::Completed => "Completed",
        RunStatus::Failed => "Failed",
        RunStatus::Cancelled => "Cancelled",
        RunStatus::Expired => "Expired",
    }
}

pub(crate) fn run_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<Run, StoreError> {
    Ok(Run {
        id: Uuid::parse_str(r.try_get("id")?)
            .map_err(|e| StoreError::InvalidUuid(e.to_string()))?,
        workflow: r.try_get("workflow")?,
        payload: serde_json::from_str(r.try_get::<&str, _>("payload")?)?,
        output: match r.try_get::<Option<String>, _>("output")? {
            Some(s) => Some(serde_json::from_str(&s)?),
            None => None,
        },
        error: r.try_get("error")?,
        status: status(r.try_get("status")?),
        attempt: r.try_get::<i64, _>("attempt")? as u32,
        max_attempts: r.try_get::<i64, _>("max_attempts")? as u32,
        next_attempt_at: dt(r.try_get("next_attempt_at")?),
        wake_at: dt(r.try_get("wake_at")?),
        wait_key: r.try_get("wait_key")?,
        wait_kind: r.try_get("wait_kind")?,
        wait_token: r.try_get("wait_token")?,
        cancel_requested: r.try_get::<i64, _>("cancel_requested")? != 0,
        concurrency_key: r.try_get("concurrency_key").ok().flatten(),
        tags: serde_json::from_str(
            r.try_get::<Option<String>, _>("tags")?
                .as_deref()
                .unwrap_or("[]"),
        )
        .unwrap_or_default(),
        metadata: serde_json::from_str(
            r.try_get::<Option<String>, _>("metadata")?
                .as_deref()
                .unwrap_or("{}"),
        )
        .unwrap_or(serde_json::json!({})),
        parent_run_id: r
            .try_get::<Option<String>, _>("parent_run_id")
            .ok()
            .flatten()
            .and_then(|v| Uuid::parse_str(&v).ok()),
        parent_wait_key: r.try_get("parent_wait_key").ok().flatten(),
        schedule_id: r.try_get("schedule_id").ok().flatten(),
        ttl_at: dt(r.try_get("ttl_at").ok().flatten()),
        source_run_id: r
            .try_get::<Option<String>, _>("source_run_id")
            .ok()
            .flatten()
            .and_then(|v| Uuid::parse_str(&v).ok()),
        leased_by: r.try_get("leased_by").ok().flatten(),
        lease_expires_at: dt(r.try_get("lease_expires_at").ok().flatten()),
        created_at: dt(Some(r.try_get::<i64, _>("created_at")?)).unwrap_or_else(Utc::now),
        updated_at: dt(Some(r.try_get::<i64, _>("updated_at")?)).unwrap_or_else(Utc::now),
    })
}

/// SQLite backend. Single-connection pool on purpose: SQLite is a
/// single-writer store, and v0.1.0 is a single-worker demo. The claim
/// transaction is atomic within that connection, so two server
/// processes must not share one database file in v0.1.0 (documented
/// limitation, not a hidden one).
#[derive(Debug, Clone)]
pub struct SqliteStore {
    pool: Pool<Sqlite>,
}

impl SqliteStore {
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let opts = url
            .parse::<SqliteConnectOptions>()
            .map_err(|e| StoreError::InvalidUuid(e.to_string()))?;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        Ok(Self { pool })
    }

    pub async fn connect_memory() -> Result<Self, StoreError> {
        Self::connect("sqlite::memory:").await
    }

    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::Store;
    use ultraflows_core::types::{NewRun, StepRecord};

    async fn store() -> SqliteStore {
        let s = SqliteStore::connect_memory().await.unwrap();
        s.migrate().await.unwrap();
        s
    }

    #[tokio::test]
    async fn step_record_is_insert_once_first_result_wins() {
        let s = store().await;
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
    async fn events_have_gapless_sequence_numbers() {
        let s = store().await;
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
    async fn claim_due_only_claims_due_runs_and_increments_attempt() {
        let s = store().await;
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
