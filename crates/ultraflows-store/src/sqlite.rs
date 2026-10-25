use crate::error::StoreError;
use crate::traits::Store;
use chrono::{DateTime, TimeZone, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Row, Sqlite};
use ultraflows_core::types::{NewRun, Run, RunEvent, RunId, RunStatus, StepRecord, WaitRecord};
use uuid::Uuid;

const SCHEMA: &str = "
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
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
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

fn ms(dt: DateTime<Utc>) -> i64 {
    dt.timestamp_millis()
}

fn dt(v: Option<i64>) -> Option<DateTime<Utc>> {
    v.and_then(|m| Utc.timestamp_millis_opt(m).single())
}

fn status(s: &str) -> RunStatus {
    match s {
        "Pending" => RunStatus::Pending,
        "Running" => RunStatus::Running,
        "Waiting" => RunStatus::Waiting,
        "Completed" => RunStatus::Completed,
        "Failed" => RunStatus::Failed,
        _ => RunStatus::Cancelled,
    }
}

fn status_str(s: RunStatus) -> &'static str {
    match s {
        RunStatus::Pending => "Pending",
        RunStatus::Running => "Running",
        RunStatus::Waiting => "Waiting",
        RunStatus::Completed => "Completed",
        RunStatus::Failed => "Failed",
        RunStatus::Cancelled => "Cancelled",
    }
}

fn run_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<Run, StoreError> {
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

impl Store for SqliteStore {
    async fn migrate(&self) -> Result<(), StoreError> {
        sqlx::query(SCHEMA).execute(&self.pool).await?;
        Ok(())
    }

    async fn create_run(&self, input: NewRun) -> Result<Run, StoreError> {
        let now = Utc::now();
        let run = Run {
            id: Uuid::now_v7(),
            workflow: input.workflow,
            payload: input.payload,
            output: None,
            error: None,
            status: RunStatus::Pending,
            attempt: 0,
            max_attempts: input.max_attempts,
            next_attempt_at: None,
            wake_at: None,
            wait_key: None,
            wait_kind: None,
            wait_token: None,
            cancel_requested: false,
            created_at: now,
            updated_at: now,
        };
        sqlx::query(
            "INSERT INTO runs (id, workflow, payload, status, attempt, max_attempts,
             cancel_requested, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(run.id.to_string())
        .bind(&run.workflow)
        .bind(serde_json::to_string(&run.payload)?)
        .bind(status_str(run.status))
        .bind(run.attempt as i64)
        .bind(run.max_attempts as i64)
        .bind(ms(now))
        .bind(ms(now))
        .execute(&self.pool)
        .await?;
        Ok(run)
    }

    async fn get_run(&self, id: RunId) -> Result<Option<Run>, StoreError> {
        let row = sqlx::query("SELECT * FROM runs WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(r) => Ok(Some(run_from_row(&r)?)),
            None => Ok(None),
        }
    }

    async fn list_runs(&self) -> Result<Vec<Run>, StoreError> {
        let rows = sqlx::query("SELECT * FROM runs ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(run_from_row).collect()
    }

    async fn claim_due(&self, now_ms: i64) -> Result<Option<Run>, StoreError> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT * FROM runs WHERE
               (status = 'Pending' AND (next_attempt_at IS NULL OR next_attempt_at <= ?))
               OR (status = 'Waiting' AND wake_at IS NOT NULL AND wake_at <= ?)
             ORDER BY created_at ASC LIMIT 1",
        )
        .bind(now_ms)
        .bind(now_ms)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(None);
        };
        let mut run = run_from_row(&row)?;
        run.status = RunStatus::Running;
        run.attempt += 1;
        run.updated_at = Utc::now();
        sqlx::query("UPDATE runs SET status = 'Running', attempt = ?, updated_at = ? WHERE id = ?")
            .bind(run.attempt as i64)
            .bind(ms(run.updated_at))
            .bind(run.id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Some(run))
    }

    async fn update_run(&self, run: &Run) -> Result<(), StoreError> {
        let n = sqlx::query(
            "UPDATE runs SET status = ?, output = ?, error = ?, attempt = ?,
             next_attempt_at = ?, wake_at = ?, wait_key = ?, wait_kind = ?,
             wait_token = ?, cancel_requested = ?, updated_at = ? WHERE id = ?",
        )
        .bind(status_str(run.status))
        .bind(run.output.as_ref().map(serde_json::to_string).transpose()?)
        .bind(&run.error)
        .bind(run.attempt as i64)
        .bind(run.next_attempt_at.map(ms))
        .bind(run.wake_at.map(ms))
        .bind(&run.wait_key)
        .bind(&run.wait_kind)
        .bind(&run.wait_token)
        .bind(i64::from(run.cancel_requested))
        .bind(ms(run.updated_at))
        .bind(run.id.to_string())
        .execute(&self.pool)
        .await?;
        if n.rows_affected() == 0 {
            return Err(StoreError::NotFound(run.id));
        }
        Ok(())
    }

    async fn record_step(&self, record: &StepRecord) -> Result<bool, StoreError> {
        let n = sqlx::query(
            "INSERT OR IGNORE INTO steps (run_id, key, result, idempotency_key, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(record.run_id.to_string())
        .bind(&record.key)
        .bind(serde_json::to_string(&record.result)?)
        .bind(&record.idempotency_key)
        .bind(ms(Utc::now()))
        .execute(&self.pool)
        .await?;
        Ok(n.rows_affected() == 1)
    }

    async fn get_step(&self, run_id: RunId, key: &str) -> Result<Option<StepRecord>, StoreError> {
        let row = sqlx::query("SELECT * FROM steps WHERE run_id = ? AND key = ?")
            .bind(run_id.to_string())
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(match row {
            Some(r) => Some(StepRecord {
                run_id,
                key: r.try_get("key")?,
                result: serde_json::from_str(r.try_get::<&str, _>("result")?)?,
                idempotency_key: r.try_get("idempotency_key")?,
            }),
            None => None,
        })
    }

    async fn list_steps(&self, run_id: RunId) -> Result<Vec<StepRecord>, StoreError> {
        let rows = sqlx::query("SELECT * FROM steps WHERE run_id = ? ORDER BY created_at ASC")
            .bind(run_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|r| {
                Ok(StepRecord {
                    run_id,
                    key: r.try_get("key")?,
                    result: serde_json::from_str(r.try_get::<&str, _>("result")?)?,
                    idempotency_key: r.try_get("idempotency_key")?,
                })
            })
            .collect()
    }

    async fn append_event(
        &self,
        run_id: RunId,
        kind: &str,
        payload: serde_json::Value,
    ) -> Result<RunEvent, StoreError> {
        let mut tx = self.pool.begin().await?;
        let seq: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) FROM events WHERE run_id = ?")
                .bind(run_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        let seq = seq + 1;
        let created_at = Utc::now();
        sqlx::query(
            "INSERT INTO events (run_id, seq, kind, payload, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(run_id.to_string())
        .bind(seq)
        .bind(kind)
        .bind(serde_json::to_string(&payload)?)
        .bind(ms(created_at))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(RunEvent {
            run_id,
            seq,
            kind: kind.to_string(),
            payload,
            created_at,
        })
    }

    async fn list_events(&self, run_id: RunId) -> Result<Vec<RunEvent>, StoreError> {
        let rows = sqlx::query("SELECT * FROM events WHERE run_id = ? ORDER BY seq ASC")
            .bind(run_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|r| {
                Ok(RunEvent {
                    run_id,
                    seq: r.try_get("seq")?,
                    kind: r.try_get("kind")?,
                    payload: serde_json::from_str(r.try_get::<&str, _>("payload")?)?,
                    created_at: dt(Some(r.try_get::<i64, _>("created_at")?))
                        .unwrap_or_else(Utc::now),
                })
            })
            .collect()
    }

    async fn upsert_wait(&self, wait: &WaitRecord) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO waits (run_id, key, kind, token, wake_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(run_id, key) DO UPDATE SET kind = excluded.kind,
               token = excluded.token, wake_at = excluded.wake_at",
        )
        .bind(wait.run_id.to_string())
        .bind(&wait.key)
        .bind(&wait.kind)
        .bind(&wait.token)
        .bind(wait.wake_at.map(ms))
        .bind(ms(Utc::now()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_wait(&self, run_id: RunId, key: &str) -> Result<Option<WaitRecord>, StoreError> {
        let row = sqlx::query("SELECT * FROM waits WHERE run_id = ? AND key = ?")
            .bind(run_id.to_string())
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(match row {
            Some(r) => Some(WaitRecord {
                run_id,
                key: r.try_get("key")?,
                kind: r.try_get("kind")?,
                token: r.try_get("token")?,
                wake_at: dt(r.try_get("wake_at")?),
            }),
            None => None,
        })
    }

    async fn resolve_wait(&self, run_id: RunId, key: &str) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM waits WHERE run_id = ? AND key = ?")
            .bind(run_id.to_string())
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_approval(&self, token: &str) -> Result<Option<serde_json::Value>, StoreError> {
        let row = sqlx::query("SELECT result FROM approvals WHERE token = ?")
            .bind(token)
            .fetch_optional(&self.pool)
            .await?;
        Ok(match row {
            Some(r) => Some(serde_json::from_str(r.try_get::<&str, _>("result")?)?),
            None => None,
        })
    }

    async fn record_approval(
        &self,
        token: &str,
        result: serde_json::Value,
        now_ms: i64,
    ) -> Result<Vec<Uuid>, StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO approvals (token, result, created_at) VALUES (?, ?, ?)
             ON CONFLICT(token) DO UPDATE SET result = excluded.result",
        )
        .bind(token)
        .bind(serde_json::to_string(&result)?)
        .bind(now_ms)
        .execute(&mut *tx)
        .await?;
        let rows = sqlx::query(
            "SELECT id FROM runs WHERE status = 'Waiting' AND wait_kind = 'approval' AND wait_token = ?",
        )
        .bind(token)
        .fetch_all(&mut *tx)
        .await?;
        let mut ids = Vec::new();
        for r in rows {
            let id = Uuid::parse_str(r.try_get("id")?)
                .map_err(|e| StoreError::InvalidUuid(e.to_string()))?;
            sqlx::query(
                "UPDATE runs SET status = 'Pending', next_attempt_at = ?, updated_at = ? WHERE id = ?",
            )
            .bind(now_ms)
            .bind(now_ms)
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;
            ids.push(id);
        }
        tx.commit().await?;
        Ok(ids)
    }

    async fn request_cancel(&self, id: RunId) -> Result<bool, StoreError> {
        let Some(run) = self.get_run(id).await? else {
            return Ok(false);
        };
        match run.status {
            RunStatus::Pending | RunStatus::Waiting => {
                sqlx::query("UPDATE runs SET status = 'Cancelled', updated_at = ? WHERE id = ?")
                    .bind(ms(Utc::now()))
                    .bind(id.to_string())
                    .execute(&self.pool)
                    .await?;
            }
            RunStatus::Running => {
                sqlx::query("UPDATE runs SET cancel_requested = 1, updated_at = ? WHERE id = ?")
                    .bind(ms(Utc::now()))
                    .bind(id.to_string())
                    .execute(&self.pool)
                    .await?;
            }
            _ => {}
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultraflows_core::types::NewRun;

    async fn store() -> SqliteStore {
        let s = SqliteStore::connect_memory().await.unwrap();
        s.migrate().await.unwrap();
        s
    }

    #[tokio::test]
    async fn step_record_is_insert_once_first_result_wins() {
        let s = store().await;
        let run = s
            .create_run(NewRun {
                workflow: "demo".to_string(),
                payload: serde_json::json!({}),
                max_attempts: 3,
            })
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
            .create_run(NewRun {
                workflow: "demo".to_string(),
                payload: serde_json::json!({}),
                max_attempts: 3,
            })
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
            .create_run(NewRun {
                workflow: "demo".to_string(),
                payload: serde_json::json!({}),
                max_attempts: 3,
            })
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
