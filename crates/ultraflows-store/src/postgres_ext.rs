//! PostgreSQL implementation of the `Store` trait (all methods in one
//! impl block; schema and struct live in `postgres.rs`).

use crate::error::StoreError;
use crate::postgres::{PG_SCHEMA, PostgresStore, pg_run_from_row};
use crate::sqlite::{dt, ms, status_str};
use crate::traits::Store;
use chrono::Utc;
use sqlx::Row;
use sqlx::types::Json;
use ultraflows_core::types::{
    IdempotencyScope, NewRun, Run, RunEvent, RunId, RunStatus, Schedule, StepRecord,
    TaskDefinition, WaitRecord,
};
use uuid::Uuid;

fn pg_task_from_row(r: &sqlx::postgres::PgRow) -> Result<TaskDefinition, StoreError> {
    Ok(TaskDefinition {
        id: r.try_get("id")?,
        version_hash: r.try_get("version_hash")?,
        retry: serde_json::from_value(r.try_get::<Json<serde_json::Value>, _>("retry_json")?.0)?,
        concurrency_total: r
            .try_get::<Option<i32>, _>("concurrency_total")?
            .map(|v| v as u32),
        concurrency_per_key: r
            .try_get::<Option<i32>, _>("concurrency_per_key")?
            .map(|v| v as u32),
        queue: r.try_get("queue")?,
        created_at: dt(Some(r.try_get::<i64, _>("created_at")?)).unwrap_or_else(Utc::now),
        updated_at: dt(Some(r.try_get::<i64, _>("updated_at")?)).unwrap_or_else(Utc::now),
    })
}

fn pg_schedule_from_row(r: &sqlx::postgres::PgRow) -> Result<Schedule, StoreError> {
    Ok(Schedule {
        id: r.try_get("id")?,
        task_id: r.try_get("task_id")?,
        cron: r.try_get("cron")?,
        timezone: r.try_get("timezone")?,
        external_id: r.try_get("external_id")?,
        deduplication_key: r.try_get("deduplication_key")?,
        payload: r.try_get::<Json<serde_json::Value>, _>("payload")?.0,
        enabled: r.try_get("enabled")?,
        next_fire_at: dt(r.try_get("next_fire_at")?),
        created_at: dt(Some(r.try_get::<i64, _>("created_at")?)).unwrap_or_else(Utc::now),
        updated_at: dt(Some(r.try_get::<i64, _>("updated_at")?)).unwrap_or_else(Utc::now),
    })
}

fn insert_run_sql() -> &'static str {
    "INSERT INTO runs (id, workflow, payload, status, attempt, max_attempts,
     next_attempt_at, cancel_requested, concurrency_key, tags, metadata,
     parent_run_id, parent_wait_key, schedule_id, ttl_at, source_run_id,
     created_at, updated_at)
     VALUES ($1, $2, $3::jsonb, $4, $5, $6, $7, FALSE, $8, $9::jsonb, $10::jsonb, $11, $12, $13, $14, $15, $16, $17)"
}

impl Store for PostgresStore {
    async fn migrate(&self) -> Result<(), StoreError> {
        sqlx::raw_sql(PG_SCHEMA).execute(self.pool()).await?;
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
            next_attempt_at: input.next_attempt_at,
            wake_at: None,
            wait_key: None,
            wait_kind: None,
            wait_token: None,
            cancel_requested: false,
            concurrency_key: input.concurrency_key,
            tags: input.tags,
            metadata: input.metadata,
            parent_run_id: input.parent_run_id,
            parent_wait_key: input.parent_wait_key,
            schedule_id: input.schedule_id,
            ttl_at: input.ttl_at,
            source_run_id: input.source_run_id,
            leased_by: None,
            lease_expires_at: None,
            created_at: now,
            updated_at: now,
        };
        sqlx::query(insert_run_sql())
            .bind(run.id)
            .bind(&run.workflow)
            .bind(Json(&run.payload))
            .bind(status_str(run.status))
            .bind(0i32)
            .bind(run.max_attempts as i32)
            .bind(run.next_attempt_at.map(ms))
            .bind(&run.concurrency_key)
            .bind(Json(&run.tags))
            .bind(Json(&run.metadata))
            .bind(run.parent_run_id)
            .bind(&run.parent_wait_key)
            .bind(&run.schedule_id)
            .bind(run.ttl_at.map(ms))
            .bind(run.source_run_id)
            .bind(ms(now))
            .bind(ms(now))
            .execute(self.pool())
            .await?;
        Ok(run)
    }

    async fn get_run(&self, id: RunId) -> Result<Option<Run>, StoreError> {
        let row = sqlx::query("SELECT * FROM runs WHERE id = $1")
            .bind(id)
            .fetch_optional(self.pool())
            .await?;
        match row {
            Some(r) => Ok(Some(pg_run_from_row(&r)?)),
            None => Ok(None),
        }
    }

    async fn list_runs(&self) -> Result<Vec<Run>, StoreError> {
        let rows = sqlx::query("SELECT * FROM runs ORDER BY created_at DESC")
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(pg_run_from_row).collect()
    }

    async fn claim_due(&self, now_ms: i64) -> Result<Option<Run>, StoreError> {
        let mut tx = self.pool().begin().await?;
        let row = sqlx::query(
            "SELECT * FROM runs WHERE
               (status = 'Pending' AND (next_attempt_at IS NULL OR next_attempt_at <= $1))
               OR (status = 'Waiting' AND wake_at IS NOT NULL AND wake_at <= $2)
             ORDER BY created_at ASC LIMIT 1 FOR UPDATE SKIP LOCKED",
        )
        .bind(now_ms)
        .bind(now_ms)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(None);
        };
        let mut run = pg_run_from_row(&row)?;
        run.status = RunStatus::Running;
        run.attempt += 1;
        run.updated_at = Utc::now();
        sqlx::query(
            "UPDATE runs SET status = 'Running', attempt = $1, leased_by = NULL,
             lease_expires_at = NULL, updated_at = $2 WHERE id = $3",
        )
        .bind(run.attempt as i32)
        .bind(ms(run.updated_at))
        .bind(run.id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(run))
    }

    async fn update_run(&self, run: &Run) -> Result<(), StoreError> {
        let n = sqlx::query(
            "UPDATE runs SET status = $1, output = $2::jsonb, error = $3, attempt = $4,
             next_attempt_at = $5, wake_at = $6, wait_key = $7, wait_kind = $8,
             wait_token = $9, cancel_requested = $10, concurrency_key = $11,
             tags = $12::jsonb, metadata = $13::jsonb, ttl_at = $14, leased_by = $15,
             lease_expires_at = $16, updated_at = $17 WHERE id = $18",
        )
        .bind(status_str(run.status))
        .bind(run.output.as_ref().map(Json))
        .bind(&run.error)
        .bind(run.attempt as i32)
        .bind(run.next_attempt_at.map(ms))
        .bind(run.wake_at.map(ms))
        .bind(&run.wait_key)
        .bind(&run.wait_kind)
        .bind(&run.wait_token)
        .bind(run.cancel_requested)
        .bind(&run.concurrency_key)
        .bind(Json(&run.tags))
        .bind(Json(&run.metadata))
        .bind(run.ttl_at.map(ms))
        .bind(&run.leased_by)
        .bind(run.lease_expires_at.map(ms))
        .bind(ms(run.updated_at))
        .bind(run.id)
        .execute(self.pool())
        .await?;
        if n.rows_affected() == 0 {
            return Err(StoreError::NotFound(run.id));
        }
        Ok(())
    }

    async fn record_step(&self, record: &StepRecord) -> Result<bool, StoreError> {
        let n = sqlx::query(
            "INSERT INTO steps (run_id, key, result, idempotency_key, created_at)
             VALUES ($1, $2, $3::jsonb, $4, $5) ON CONFLICT DO NOTHING",
        )
        .bind(record.run_id)
        .bind(&record.key)
        .bind(Json(&record.result))
        .bind(&record.idempotency_key)
        .bind(ms(Utc::now()))
        .execute(self.pool())
        .await?;
        Ok(n.rows_affected() == 1)
    }

    async fn get_step(&self, run_id: RunId, key: &str) -> Result<Option<StepRecord>, StoreError> {
        let row = sqlx::query("SELECT * FROM steps WHERE run_id = $1 AND key = $2")
            .bind(run_id)
            .bind(key)
            .fetch_optional(self.pool())
            .await?;
        Ok(match row {
            Some(r) => Some(StepRecord {
                run_id,
                key: r.try_get("key")?,
                result: r.try_get::<Json<serde_json::Value>, _>("result")?.0,
                idempotency_key: r.try_get("idempotency_key")?,
            }),
            None => None,
        })
    }

    async fn list_steps(&self, run_id: RunId) -> Result<Vec<StepRecord>, StoreError> {
        let rows = sqlx::query("SELECT * FROM steps WHERE run_id = $1 ORDER BY created_at ASC")
            .bind(run_id)
            .fetch_all(self.pool())
            .await?;
        rows.iter()
            .map(|r| {
                Ok(StepRecord {
                    run_id,
                    key: r.try_get("key")?,
                    result: r.try_get::<Json<serde_json::Value>, _>("result")?.0,
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
        let mut tx = self.pool().begin().await?;
        // Atomic gapless sequence: the counter row is locked by the
        // upsert for the duration of this transaction.
        let seq: i64 = sqlx::query_scalar(
            "INSERT INTO event_counters (run_id, last_seq) VALUES ($1, 1)
             ON CONFLICT (run_id) DO UPDATE
               SET last_seq = event_counters.last_seq + 1
             RETURNING last_seq",
        )
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;
        let created_at = Utc::now();
        sqlx::query(
            "INSERT INTO events (run_id, seq, kind, payload, created_at)
             VALUES ($1, $2, $3, $4::jsonb, $5)",
        )
        .bind(run_id)
        .bind(seq)
        .bind(kind)
        .bind(Json(&payload))
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
        let rows = sqlx::query("SELECT * FROM events WHERE run_id = $1 ORDER BY seq ASC")
            .bind(run_id)
            .fetch_all(self.pool())
            .await?;
        rows.iter()
            .map(|r| {
                Ok(RunEvent {
                    run_id,
                    seq: r.try_get("seq")?,
                    kind: r.try_get("kind")?,
                    payload: r.try_get::<Json<serde_json::Value>, _>("payload")?.0,
                    created_at: dt(Some(r.try_get::<i64, _>("created_at")?))
                        .unwrap_or_else(Utc::now),
                })
            })
            .collect()
    }

    async fn upsert_wait(&self, wait: &WaitRecord) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO waits (run_id, key, kind, token, wake_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (run_id, key) DO UPDATE SET kind = EXCLUDED.kind,
               token = EXCLUDED.token, wake_at = EXCLUDED.wake_at",
        )
        .bind(wait.run_id)
        .bind(&wait.key)
        .bind(&wait.kind)
        .bind(&wait.token)
        .bind(wait.wake_at.map(ms))
        .bind(ms(Utc::now()))
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn get_wait(&self, run_id: RunId, key: &str) -> Result<Option<WaitRecord>, StoreError> {
        let row = sqlx::query("SELECT * FROM waits WHERE run_id = $1 AND key = $2")
            .bind(run_id)
            .bind(key)
            .fetch_optional(self.pool())
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
        sqlx::query("DELETE FROM waits WHERE run_id = $1 AND key = $2")
            .bind(run_id)
            .bind(key)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn get_approval(&self, token: &str) -> Result<Option<serde_json::Value>, StoreError> {
        let row = sqlx::query("SELECT result FROM approvals WHERE token = $1")
            .bind(token)
            .fetch_optional(self.pool())
            .await?;
        Ok(match row {
            Some(r) => Some(r.try_get::<Json<serde_json::Value>, _>("result")?.0),
            None => None,
        })
    }

    async fn record_approval(
        &self,
        token: &str,
        result: serde_json::Value,
        now_ms: i64,
    ) -> Result<Vec<Uuid>, StoreError> {
        let mut tx = self.pool().begin().await?;
        sqlx::query(
            "INSERT INTO approvals (token, result, created_at) VALUES ($1, $2::jsonb, $3)
             ON CONFLICT (token) DO UPDATE SET result = EXCLUDED.result",
        )
        .bind(token)
        .bind(Json(&result))
        .bind(now_ms)
        .execute(&mut *tx)
        .await?;
        let rows = sqlx::query(
            "SELECT id FROM runs WHERE status = 'Waiting' AND wait_kind = 'approval' AND wait_token = $1",
        )
        .bind(token)
        .fetch_all(&mut *tx)
        .await?;
        let mut ids = Vec::new();
        for r in rows {
            let id: Uuid = r.try_get("id")?;
            sqlx::query(
                "UPDATE runs SET status = 'Pending', next_attempt_at = $1, updated_at = $2 WHERE id = $3",
            )
            .bind(now_ms)
            .bind(now_ms)
            .bind(id)
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
                sqlx::query("UPDATE runs SET status = 'Cancelled', updated_at = $1 WHERE id = $2")
                    .bind(ms(Utc::now()))
                    .bind(id)
                    .execute(self.pool())
                    .await?;
            }
            RunStatus::Running => {
                sqlx::query(
                    "UPDATE runs SET cancel_requested = TRUE, updated_at = $1 WHERE id = $2",
                )
                .bind(ms(Utc::now()))
                .bind(id)
                .execute(self.pool())
                .await?;
            }
            _ => {}
        }
        Ok(true)
    }

    async fn touch_worker(&self, worker_id: &str, now_ms: i64) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO workers (id, last_seen) VALUES ($1, $2)
             ON CONFLICT (id) DO UPDATE SET last_seen = EXCLUDED.last_seen",
        )
        .bind(worker_id)
        .bind(now_ms)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    // ---- Task registry ----

    async fn upsert_task(&self, task: &TaskDefinition) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO tasks (id, version_hash, retry_json, concurrency_total,
             concurrency_per_key, queue, created_at, updated_at)
             VALUES ($1, $2, $3::jsonb, $4, $5, $6, $7, $8)
             ON CONFLICT (id) DO UPDATE SET version_hash = EXCLUDED.version_hash,
               retry_json = EXCLUDED.retry_json,
               concurrency_total = EXCLUDED.concurrency_total,
               concurrency_per_key = EXCLUDED.concurrency_per_key,
               queue = EXCLUDED.queue, updated_at = EXCLUDED.updated_at",
        )
        .bind(&task.id)
        .bind(&task.version_hash)
        .bind(Json(&task.retry))
        .bind(task.concurrency_total.map(|v| v as i32))
        .bind(task.concurrency_per_key.map(|v| v as i32))
        .bind(&task.queue)
        .bind(ms(task.created_at))
        .bind(ms(task.updated_at))
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn get_task(&self, id: &str) -> Result<Option<TaskDefinition>, StoreError> {
        let row = sqlx::query("SELECT * FROM tasks WHERE id = $1")
            .bind(id)
            .fetch_optional(self.pool())
            .await?;
        match row {
            Some(r) => Ok(Some(pg_task_from_row(&r)?)),
            None => Ok(None),
        }
    }

    async fn list_tasks(&self) -> Result<Vec<TaskDefinition>, StoreError> {
        let rows = sqlx::query("SELECT * FROM tasks ORDER BY id ASC")
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(pg_task_from_row).collect()
    }

    async fn create_run_idempotent(
        &self,
        input: NewRun,
        scope: IdempotencyScope,
        key: &str,
        expires_at_ms: i64,
    ) -> Result<(Run, bool), StoreError> {
        let (scope_str, storage_key) = scope.storage_key(&input.workflow, key);
        let mut tx = self.pool().begin().await?;
        // Serialize concurrent triggers for the same key; different
        // keys hash to different locks and proceed in parallel.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(format!("{scope_str}:{storage_key}"))
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM idempotency_keys WHERE expires_at <= $1")
            .bind(Utc::now().timestamp_millis())
            .execute(&mut *tx)
            .await?;
        if let Some(row) =
            sqlx::query("SELECT run_id FROM idempotency_keys WHERE scope = $1 AND key = $2")
                .bind(&scope_str)
                .bind(&storage_key)
                .fetch_optional(&mut *tx)
                .await?
        {
            let run_id: Uuid = row.try_get("run_id")?;
            let run_row = sqlx::query("SELECT * FROM runs WHERE id = $1")
                .bind(run_id)
                .fetch_optional(&mut *tx)
                .await?;
            tx.commit().await?;
            return match run_row {
                Some(r) => Ok((pg_run_from_row(&r)?, false)),
                None => Err(StoreError::NotFound(run_id)),
            };
        }
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
            next_attempt_at: input.next_attempt_at,
            wake_at: None,
            wait_key: None,
            wait_kind: None,
            wait_token: None,
            cancel_requested: false,
            concurrency_key: input.concurrency_key,
            tags: input.tags,
            metadata: input.metadata,
            parent_run_id: input.parent_run_id,
            parent_wait_key: input.parent_wait_key,
            schedule_id: input.schedule_id,
            ttl_at: input.ttl_at,
            source_run_id: input.source_run_id,
            leased_by: None,
            lease_expires_at: None,
            created_at: now,
            updated_at: now,
        };
        sqlx::query(insert_run_sql())
            .bind(run.id)
            .bind(&run.workflow)
            .bind(Json(&run.payload))
            .bind(status_str(run.status))
            .bind(0i32)
            .bind(run.max_attempts as i32)
            .bind(run.next_attempt_at.map(ms))
            .bind(&run.concurrency_key)
            .bind(Json(&run.tags))
            .bind(Json(&run.metadata))
            .bind(run.parent_run_id)
            .bind(&run.parent_wait_key)
            .bind(&run.schedule_id)
            .bind(run.ttl_at.map(ms))
            .bind(run.source_run_id)
            .bind(ms(now))
            .bind(ms(now))
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO idempotency_keys (scope, key, run_id, expires_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(&scope_str)
        .bind(&storage_key)
        .bind(run.id)
        .bind(expires_at_ms)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok((run, true))
    }

    async fn list_runs_filtered(
        &self,
        status: Option<&str>,
        task: Option<&str>,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
    ) -> Result<Vec<Run>, StoreError> {
        let mut sql = "SELECT * FROM runs WHERE 1 = 1".to_string();
        let mut n = 0;
        if status.is_some() {
            n += 1;
            sql.push_str(&format!(" AND status = ${n}"));
        }
        if task.is_some() {
            n += 1;
            sql.push_str(&format!(" AND workflow = ${n}"));
        }
        if from_ms.is_some() {
            n += 1;
            sql.push_str(&format!(" AND created_at >= ${n}"));
        }
        if to_ms.is_some() {
            n += 1;
            sql.push_str(&format!(" AND created_at <= ${n}"));
        }
        sql.push_str(" ORDER BY created_at DESC");
        let mut q = sqlx::query(&sql);
        if let Some(s) = status {
            q = q.bind(s);
        }
        if let Some(t) = task {
            q = q.bind(t);
        }
        if let Some(f) = from_ms {
            q = q.bind(f);
        }
        if let Some(t) = to_ms {
            q = q.bind(t);
        }
        let rows = q.fetch_all(self.pool()).await?;
        rows.iter().map(pg_run_from_row).collect()
    }

    async fn list_due(&self, now_ms: i64) -> Result<Vec<Run>, StoreError> {
        let rows = sqlx::query(
            "SELECT * FROM runs WHERE
               (status = 'Pending' AND (next_attempt_at IS NULL OR next_attempt_at <= $1))
               OR (status = 'Waiting' AND wake_at IS NOT NULL AND wake_at <= $2)
             ORDER BY created_at ASC",
        )
        .bind(now_ms)
        .bind(now_ms)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(pg_run_from_row).collect()
    }

    async fn claim_specific(
        &self,
        run_id: RunId,
        worker_id: &str,
        now_ms: i64,
    ) -> Result<Option<Run>, StoreError> {
        let mut tx = self.pool().begin().await?;
        let row = sqlx::query("SELECT * FROM runs WHERE id = $1 FOR UPDATE SKIP LOCKED")
            .bind(run_id)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(None);
        };
        let mut run = pg_run_from_row(&row)?;
        let claimable = match run.status {
            RunStatus::Pending => run
                .next_attempt_at
                .map(ms)
                .map(|t| t <= now_ms)
                .unwrap_or(true),
            RunStatus::Waiting => run.wake_at.map(|w| ms(w) <= now_ms).unwrap_or(false),
            RunStatus::Running => run
                .lease_expires_at
                .map(|l| ms(l) <= now_ms)
                .unwrap_or(false),
            _ => false,
        };
        if !claimable {
            tx.rollback().await?;
            return Ok(None);
        }
        run.status = RunStatus::Running;
        run.attempt += 1;
        run.leased_by = Some(worker_id.to_string());
        run.lease_expires_at = dt(Some(now_ms + 30_000));
        run.updated_at = Utc::now();
        sqlx::query(
            "UPDATE runs SET status = 'Running', attempt = $1, leased_by = $2,
             lease_expires_at = $3, updated_at = $4 WHERE id = $5",
        )
        .bind(run.attempt as i32)
        .bind(worker_id)
        .bind(now_ms + 30_000)
        .bind(ms(run.updated_at))
        .bind(run.id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(run))
    }

    async fn count_running_total(&self, task_id: &str) -> Result<u32, StoreError> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM runs WHERE workflow = $1 AND status = 'Running'",
        )
        .bind(task_id)
        .fetch_one(self.pool())
        .await?;
        Ok(n as u32)
    }

    async fn count_running_for_key(&self, task_id: &str, key: &str) -> Result<u32, StoreError> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM runs WHERE workflow = $1 AND status = 'Running' AND concurrency_key = $2",
        )
        .bind(task_id)
        .bind(key)
        .fetch_one(self.pool())
        .await?;
        Ok(n as u32)
    }

    async fn expire_ttl(&self, now_ms: i64) -> Result<Vec<Uuid>, StoreError> {
        let rows = sqlx::query(
            "SELECT id FROM runs WHERE status = 'Pending' AND attempt = 0
             AND ttl_at IS NOT NULL AND ttl_at <= $1",
        )
        .bind(now_ms)
        .fetch_all(self.pool())
        .await?;
        let mut ids = Vec::new();
        for r in rows {
            let id: Uuid = r.try_get("id")?;
            sqlx::query("UPDATE runs SET status = 'Expired', updated_at = $1 WHERE id = $2")
                .bind(now_ms)
                .bind(id)
                .execute(self.pool())
                .await?;
            self.append_event(id, "Expired", serde_json::json!({"reason": "ttl"}))
                .await?;
            ids.push(id);
        }
        Ok(ids)
    }

    async fn promote_pending_version(&self, task_id: &str) -> Result<Vec<Uuid>, StoreError> {
        let rows =
            sqlx::query("SELECT id FROM runs WHERE workflow = $1 AND status = 'PendingVersion'")
                .bind(task_id)
                .fetch_all(self.pool())
                .await?;
        let mut ids = Vec::new();
        for r in rows {
            let id: Uuid = r.try_get("id")?;
            sqlx::query("UPDATE runs SET status = 'Pending', updated_at = $1 WHERE id = $2")
                .bind(Utc::now().timestamp_millis())
                .bind(id)
                .execute(self.pool())
                .await?;
            self.append_event(id, "Promoted", serde_json::json!({"task": task_id}))
                .await?;
            ids.push(id);
        }
        Ok(ids)
    }

    async fn suspend_run(&self, run_id: RunId) -> Result<bool, StoreError> {
        let n = sqlx::query(
            "UPDATE runs SET status = 'Waiting', leased_by = NULL, lease_expires_at = NULL,
             updated_at = $1 WHERE id = $2 AND status = 'Running'",
        )
        .bind(Utc::now().timestamp_millis())
        .bind(run_id)
        .execute(self.pool())
        .await?;
        Ok(n.rows_affected() == 1)
    }

    async fn wake_child_parents(
        &self,
        child_id: RunId,
        now_ms: i64,
    ) -> Result<Vec<Uuid>, StoreError> {
        let rows = sqlx::query(
            "SELECT id FROM runs WHERE status = 'Waiting' AND wait_kind = 'child'
             AND wait_token = $1",
        )
        .bind(child_id.to_string())
        .fetch_all(self.pool())
        .await?;
        let mut ids = Vec::new();
        for r in rows {
            let id: Uuid = r.try_get("id")?;
            sqlx::query(
                "UPDATE runs SET status = 'Pending', next_attempt_at = $1, updated_at = $2 WHERE id = $3",
            )
            .bind(now_ms)
            .bind(now_ms)
            .bind(id)
            .execute(self.pool())
            .await?;
            ids.push(id);
        }
        Ok(ids)
    }

    // ---- Schedules ----

    async fn create_schedule(&self, schedule: &Schedule) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO schedules (id, task_id, cron, timezone, external_id,
             deduplication_key, payload, enabled, next_fire_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, $8, $9, $10, $11)",
        )
        .bind(&schedule.id)
        .bind(&schedule.task_id)
        .bind(&schedule.cron)
        .bind(&schedule.timezone)
        .bind(&schedule.external_id)
        .bind(&schedule.deduplication_key)
        .bind(Json(&schedule.payload))
        .bind(schedule.enabled)
        .bind(schedule.next_fire_at.map(ms))
        .bind(ms(schedule.created_at))
        .bind(ms(schedule.updated_at))
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn upsert_schedule_by_dedup(&self, schedule: &Schedule) -> Result<Schedule, StoreError> {
        if let Some(key) = &schedule.deduplication_key {
            if let Some(row) = sqlx::query("SELECT * FROM schedules WHERE deduplication_key = $1")
                .bind(key)
                .fetch_optional(self.pool())
                .await?
            {
                let mut existing = pg_schedule_from_row(&row)?;
                existing.task_id = schedule.task_id.clone();
                existing.cron = schedule.cron.clone();
                existing.timezone = schedule.timezone.clone();
                existing.external_id = schedule.external_id.clone();
                existing.payload = schedule.payload.clone();
                existing.enabled = schedule.enabled;
                existing.next_fire_at = schedule.next_fire_at;
                existing.updated_at = Utc::now();
                self.update_schedule(&existing).await?;
                return Ok(existing);
            }
        }
        self.create_schedule(schedule).await?;
        Ok(schedule.clone())
    }

    async fn get_schedule(&self, id: &str) -> Result<Option<Schedule>, StoreError> {
        let row = sqlx::query("SELECT * FROM schedules WHERE id = $1")
            .bind(id)
            .fetch_optional(self.pool())
            .await?;
        match row {
            Some(r) => Ok(Some(pg_schedule_from_row(&r)?)),
            None => Ok(None),
        }
    }

    async fn list_schedules(&self) -> Result<Vec<Schedule>, StoreError> {
        let rows = sqlx::query("SELECT * FROM schedules ORDER BY created_at DESC")
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(pg_schedule_from_row).collect()
    }

    async fn update_schedule(&self, schedule: &Schedule) -> Result<(), StoreError> {
        let n = sqlx::query(
            "UPDATE schedules SET task_id = $1, cron = $2, timezone = $3, external_id = $4,
             deduplication_key = $5, payload = $6::jsonb, enabled = $7, next_fire_at = $8,
             updated_at = $9 WHERE id = $10",
        )
        .bind(&schedule.task_id)
        .bind(&schedule.cron)
        .bind(&schedule.timezone)
        .bind(&schedule.external_id)
        .bind(&schedule.deduplication_key)
        .bind(Json(&schedule.payload))
        .bind(schedule.enabled)
        .bind(schedule.next_fire_at.map(ms))
        .bind(ms(schedule.updated_at))
        .bind(&schedule.id)
        .execute(self.pool())
        .await?;
        if n.rows_affected() == 0 {
            return Err(StoreError::NotFound(Uuid::nil()));
        }
        Ok(())
    }

    async fn delete_schedule(&self, id: &str) -> Result<bool, StoreError> {
        let n = sqlx::query("DELETE FROM schedules WHERE id = $1")
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(n.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::postgres::tests::pg_store;
    use ultraflows_core::RetryPolicy;

    /// All Postgres conformance checks in ONE test function, for
    /// readability (each test gets its own fresh database from
    /// `pg_store`, so parallel tests no longer interfere).
    /// Skips (passes) when no test URL is configured.
    #[tokio::test]
    async fn pg_conformance() {
        let Some(s) = pg_store().await else {
            return;
        };
        let now = Utc::now().timestamp_millis();

        // Run lifecycle + filtered listing.
        let run = s
            .create_run(NewRun::new("task-a", serde_json::json!({"x": 1}), 3))
            .await
            .unwrap();
        assert_eq!(
            s.list_runs_filtered(Some("Pending"), Some("task-a"), None, None)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            s.list_runs_filtered(Some("Completed"), None, None, None)
                .await
                .unwrap()
                .len(),
            0
        );

        // Idempotent trigger: same key -> same run, first payload wins;
        // global scope dedupes across tasks.
        let (r1, c1) = s
            .create_run_idempotent(
                NewRun::new("task-a", serde_json::json!({"x": 1}), 3),
                IdempotencyScope::Run,
                "key-1",
                now + 86_400_000,
            )
            .await
            .unwrap();
        let (r2, c2) = s
            .create_run_idempotent(
                NewRun::new("task-a", serde_json::json!({"x": 999}), 3),
                IdempotencyScope::Run,
                "key-1",
                now + 86_400_000,
            )
            .await
            .unwrap();
        assert!(c1 && !c2);
        assert_eq!(r1.id, r2.id);
        assert_eq!(r2.payload["x"], 1);

        // Claim concurrency: claim_specific leases, second claim while
        // leased fails, claim after lease expiry succeeds.
        let claimed = s
            .claim_specific(run.id, "worker-1", now)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claimed.attempt, 1);
        assert!(
            s.claim_specific(run.id, "worker-2", now)
                .await
                .unwrap()
                .is_none()
        );
        let claimed2 = s
            .claim_specific(run.id, "worker-2", now + 31_000)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claimed2.attempt, 2);
        // Counts: one Running run for task-a.
        assert_eq!(s.count_running_total("task-a").await.unwrap(), 1);

        // Cancel: Running -> flag only; Pending -> Cancelled.
        assert!(s.request_cancel(run.id).await.unwrap());
        let flagged = s.get_run(run.id).await.unwrap().unwrap();
        assert!(flagged.cancel_requested);
        assert!(s.request_cancel(r1.id).await.unwrap());
        assert_eq!(
            s.get_run(r1.id).await.unwrap().unwrap().status,
            RunStatus::Cancelled
        );

        // TTL expiry: only never-started Pending runs.
        let mut ttl_input = NewRun::new("task-a", serde_json::json!({}), 3);
        ttl_input.ttl_at = Some(Utc::now() - chrono::Duration::seconds(1));
        let ttl_run = s.create_run(ttl_input).await.unwrap();
        let expired = s.expire_ttl(now).await.unwrap();
        assert_eq!(expired, vec![ttl_run.id]);
        assert_eq!(
            s.get_run(ttl_run.id).await.unwrap().unwrap().status,
            RunStatus::Expired
        );

        // Child wake: parent suspended on child -> Pending.
        let child = s
            .create_run(NewRun::new("child-task", serde_json::json!({}), 3))
            .await
            .unwrap();
        let parent = s
            .create_run(NewRun::new("parent-task", serde_json::json!({}), 3))
            .await
            .unwrap();
        let mut p = s.get_run(parent.id).await.unwrap().unwrap();
        p.status = RunStatus::Waiting;
        p.wait_kind = Some("child".to_string());
        p.wait_token = Some(child.id.to_string());
        s.update_run(&p).await.unwrap();
        assert_eq!(
            s.wake_child_parents(child.id, now).await.unwrap(),
            vec![parent.id]
        );

        // Task registry + schedule dedup.
        let task = TaskDefinition {
            id: "task-a".to_string(),
            version_hash: "abc123".to_string(),
            retry: RetryPolicy::default(),
            concurrency_total: Some(3),
            concurrency_per_key: Some(1),
            queue: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        s.upsert_task(&task).await.unwrap();
        assert_eq!(
            s.get_task("task-a")
                .await
                .unwrap()
                .unwrap()
                .retry
                .max_attempts,
            5
        );
        let base = Schedule {
            id: "sched-1".to_string(),
            task_id: "task-a".to_string(),
            cron: "0 9 * * *".to_string(),
            timezone: "UTC".to_string(),
            external_id: None,
            deduplication_key: Some("tenant-1-daily".to_string()),
            payload: serde_json::json!({}),
            enabled: true,
            next_fire_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let first = s.upsert_schedule_by_dedup(&base).await.unwrap();
        let mut second = base.clone();
        second.cron = "0 10 * * *".to_string();
        let upserted = s.upsert_schedule_by_dedup(&second).await.unwrap();
        assert_eq!(first.id, upserted.id);
        assert_eq!(upserted.cron, "0 10 * * *");
        assert_eq!(s.list_schedules().await.unwrap().len(), 1);

        // Waits + approval wake.
        let wait = WaitRecord {
            run_id: parent.id,
            key: "approve".to_string(),
            kind: "approval".to_string(),
            token: Some("tok-1".to_string()),
            wake_at: None,
        };
        s.upsert_wait(&wait).await.unwrap();
        assert_eq!(
            s.get_wait(parent.id, "approve")
                .await
                .unwrap()
                .unwrap()
                .token
                .as_deref(),
            Some("tok-1")
        );
        s.resolve_wait(parent.id, "approve").await.unwrap();
        assert!(s.get_wait(parent.id, "approve").await.unwrap().is_none());
        s.touch_worker("worker-1", now).await.unwrap();
        s.touch_worker("worker-1", now + 1000).await.unwrap();

        // PendingVersion promotion.
        let pv = s
            .create_run(NewRun::new("task-a", serde_json::json!({}), 3))
            .await
            .unwrap();
        let mut pv_run = s.get_run(pv.id).await.unwrap().unwrap();
        pv_run.status = RunStatus::PendingVersion;
        s.update_run(&pv_run).await.unwrap();
        assert_eq!(
            s.promote_pending_version("task-a").await.unwrap(),
            vec![pv.id]
        );
        // list_due includes the promoted run (Pending, no delay).
        assert!(s.list_due(now).await.unwrap().iter().any(|r| r.id == pv.id));
    }
}
