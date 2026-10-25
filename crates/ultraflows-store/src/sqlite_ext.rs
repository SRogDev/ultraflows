//! Trigger-core extensions of the SQLite store: task registry,
//! idempotent triggers, worker leasing, TTL expiry, schedules and
//! child-parent waking. Kept in a separate impl block so the original
//! v0.1.0 store code stays readable.

use crate::error::StoreError;
use crate::sqlite::{SCHEMA, SqliteStore, dt, ms, run_from_row, status_str};
use crate::traits::Store;
use chrono::Utc;
use sqlx::Row;
use ultraflows_core::types::{
    IdempotencyScope, NewRun, Run, RunEvent, RunId, RunStatus, Schedule, StepRecord,
    TaskDefinition, WaitRecord,
};
use uuid::Uuid;

fn task_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<TaskDefinition, StoreError> {
    Ok(TaskDefinition {
        id: r.try_get("id")?,
        version_hash: r.try_get("version_hash")?,
        retry: serde_json::from_str(r.try_get::<&str, _>("retry_json")?)?,
        concurrency_total: r
            .try_get::<Option<i64>, _>("concurrency_total")?
            .map(|v| v as u32),
        concurrency_per_key: r
            .try_get::<Option<i64>, _>("concurrency_per_key")?
            .map(|v| v as u32),
        queue: r.try_get("queue")?,
        created_at: dt(Some(r.try_get::<i64, _>("created_at")?)).unwrap_or_else(Utc::now),
        updated_at: dt(Some(r.try_get::<i64, _>("updated_at")?)).unwrap_or_else(Utc::now),
    })
}

fn schedule_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<Schedule, StoreError> {
    Ok(Schedule {
        id: r.try_get("id")?,
        task_id: r.try_get("task_id")?,
        cron: r.try_get("cron")?,
        timezone: r.try_get("timezone")?,
        external_id: r.try_get("external_id")?,
        deduplication_key: r.try_get("deduplication_key")?,
        payload: serde_json::from_str(r.try_get::<&str, _>("payload")?)?,
        enabled: r.try_get::<i64, _>("enabled")? != 0,
        next_fire_at: dt(r.try_get("next_fire_at")?),
        created_at: dt(Some(r.try_get::<i64, _>("created_at")?)).unwrap_or_else(Utc::now),
        updated_at: dt(Some(r.try_get::<i64, _>("updated_at")?)).unwrap_or_else(Utc::now),
    })
}

impl Store for SqliteStore {
    async fn upsert_task(&self, task: &TaskDefinition) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO tasks (id, version_hash, retry_json, concurrency_total,
             concurrency_per_key, queue, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET version_hash = excluded.version_hash,
               retry_json = excluded.retry_json,
               concurrency_total = excluded.concurrency_total,
               concurrency_per_key = excluded.concurrency_per_key,
               queue = excluded.queue, updated_at = excluded.updated_at",
        )
        .bind(&task.id)
        .bind(&task.version_hash)
        .bind(serde_json::to_string(&task.retry)?)
        .bind(task.concurrency_total.map(i64::from))
        .bind(task.concurrency_per_key.map(i64::from))
        .bind(&task.queue)
        .bind(ms(task.created_at))
        .bind(ms(task.updated_at))
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn get_task(&self, id: &str) -> Result<Option<TaskDefinition>, StoreError> {
        let row = sqlx::query("SELECT * FROM tasks WHERE id = ?")
            .bind(id)
            .fetch_optional(self.pool())
            .await?;
        match row {
            Some(r) => Ok(Some(task_from_row(&r)?)),
            None => Ok(None),
        }
    }

    async fn list_tasks(&self) -> Result<Vec<TaskDefinition>, StoreError> {
        let rows = sqlx::query("SELECT * FROM tasks ORDER BY id ASC")
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(task_from_row).collect()
    }

    async fn create_run_idempotent(
        &self,
        input: NewRun,
        scope: IdempotencyScope,
        key: &str,
        expires_at_ms: i64,
    ) -> Result<(Run, bool), StoreError> {
        let (scope_str, storage_key) = scope.storage_key(&input.workflow, key);
        // BEGIN IMMEDIATE serializes writers on this single-connection
        // pool: check + insert is atomic for the single-node server.
        let mut tx = self.pool().begin().await?;
        sqlx::query("DELETE FROM idempotency_keys WHERE expires_at <= ?")
            .bind(Utc::now().timestamp_millis())
            .execute(&mut *tx)
            .await?;
        if let Some(row) =
            sqlx::query("SELECT run_id FROM idempotency_keys WHERE scope = ? AND key = ?")
                .bind(&scope_str)
                .bind(&storage_key)
                .fetch_optional(&mut *tx)
                .await?
        {
            let run_id = Uuid::parse_str(row.try_get("run_id")?)
                .map_err(|e| StoreError::InvalidUuid(e.to_string()))?;
            let run_row = sqlx::query("SELECT * FROM runs WHERE id = ?")
                .bind(run_id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
            tx.commit().await?;
            return match run_row {
                Some(r) => Ok((run_from_row(&r)?, false)),
                None => Err(StoreError::NotFound(run_id)),
            };
        }
        // Reuse create_run's insert by dropping the transaction's
        // connection discipline: create the run first via a fresh
        // insert inside this transaction.
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
        sqlx::query(
            "INSERT INTO runs (id, workflow, payload, status, attempt, max_attempts,
             next_attempt_at, cancel_requested, concurrency_key, tags, metadata,
             parent_run_id, parent_wait_key, schedule_id, ttl_at, source_run_id,
             created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(run.id.to_string())
        .bind(&run.workflow)
        .bind(serde_json::to_string(&run.payload)?)
        .bind(status_str(run.status))
        .bind(0i64)
        .bind(run.max_attempts as i64)
        .bind(run.next_attempt_at.map(ms))
        .bind(&run.concurrency_key)
        .bind(serde_json::to_string(&run.tags)?)
        .bind(serde_json::to_string(&run.metadata)?)
        .bind(run.parent_run_id.map(|u| u.to_string()))
        .bind(&run.parent_wait_key)
        .bind(&run.schedule_id)
        .bind(run.ttl_at.map(ms))
        .bind(run.source_run_id.map(|u| u.to_string()))
        .bind(ms(now))
        .bind(ms(now))
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO idempotency_keys (scope, key, run_id, expires_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&scope_str)
        .bind(&storage_key)
        .bind(run.id.to_string())
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
        if status.is_some() {
            sql.push_str(" AND status = ?");
        }
        if task.is_some() {
            sql.push_str(" AND workflow = ?");
        }
        if from_ms.is_some() {
            sql.push_str(" AND created_at >= ?");
        }
        if to_ms.is_some() {
            sql.push_str(" AND created_at <= ?");
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
        rows.iter().map(run_from_row).collect()
    }

    async fn list_due(&self, now_ms: i64) -> Result<Vec<Run>, StoreError> {
        let rows = sqlx::query(
            "SELECT * FROM runs WHERE
               (status = 'Pending' AND (next_attempt_at IS NULL OR next_attempt_at <= ?))
               OR (status = 'Waiting' AND wake_at IS NOT NULL AND wake_at <= ?)
             ORDER BY created_at ASC",
        )
        .bind(now_ms)
        .bind(now_ms)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(run_from_row).collect()
    }

    async fn claim_specific(
        &self,
        run_id: RunId,
        worker_id: &str,
        now_ms: i64,
    ) -> Result<Option<Run>, StoreError> {
        let mut tx = self.pool().begin().await?;
        let row = sqlx::query("SELECT * FROM runs WHERE id = ?")
            .bind(run_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(None);
        };
        let mut run = run_from_row(&row)?;
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
            "UPDATE runs SET status = 'Running', attempt = ?, leased_by = ?,
             lease_expires_at = ?, updated_at = ? WHERE id = ?",
        )
        .bind(run.attempt as i64)
        .bind(worker_id)
        .bind(now_ms + 30_000)
        .bind(ms(run.updated_at))
        .bind(run.id.to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(run))
    }

    async fn count_running_total(&self, task_id: &str) -> Result<u32, StoreError> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM runs WHERE workflow = ? AND status = 'Running'",
        )
        .bind(task_id)
        .fetch_one(self.pool())
        .await?;
        Ok(n as u32)
    }

    async fn count_running_for_key(&self, task_id: &str, key: &str) -> Result<u32, StoreError> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM runs WHERE workflow = ? AND status = 'Running' AND concurrency_key = ?",
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
             AND ttl_at IS NOT NULL AND ttl_at <= ?",
        )
        .bind(now_ms)
        .fetch_all(self.pool())
        .await?;
        let mut ids = Vec::new();
        for r in rows {
            let id = Uuid::parse_str(r.try_get("id")?)
                .map_err(|e| StoreError::InvalidUuid(e.to_string()))?;
            sqlx::query("UPDATE runs SET status = 'Expired', updated_at = ? WHERE id = ?")
                .bind(now_ms)
                .bind(id.to_string())
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
            sqlx::query("SELECT id FROM runs WHERE workflow = ? AND status = 'PendingVersion'")
                .bind(task_id)
                .fetch_all(self.pool())
                .await?;
        let mut ids = Vec::new();
        for r in rows {
            let id = Uuid::parse_str(r.try_get("id")?)
                .map_err(|e| StoreError::InvalidUuid(e.to_string()))?;
            sqlx::query("UPDATE runs SET status = 'Pending', updated_at = ? WHERE id = ?")
                .bind(Utc::now().timestamp_millis())
                .bind(id.to_string())
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
             updated_at = ? WHERE id = ? AND status = 'Running'",
        )
        .bind(Utc::now().timestamp_millis())
        .bind(run_id.to_string())
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
             AND wait_token = ?",
        )
        .bind(child_id.to_string())
        .fetch_all(self.pool())
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
            .execute(self.pool())
            .await?;
            ids.push(id);
        }
        Ok(ids)
    }

    async fn create_schedule(&self, schedule: &Schedule) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO schedules (id, task_id, cron, timezone, external_id,
             deduplication_key, payload, enabled, next_fire_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&schedule.id)
        .bind(&schedule.task_id)
        .bind(&schedule.cron)
        .bind(&schedule.timezone)
        .bind(&schedule.external_id)
        .bind(&schedule.deduplication_key)
        .bind(serde_json::to_string(&schedule.payload)?)
        .bind(i64::from(schedule.enabled))
        .bind(schedule.next_fire_at.map(ms))
        .bind(ms(schedule.created_at))
        .bind(ms(schedule.updated_at))
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn upsert_schedule_by_dedup(&self, schedule: &Schedule) -> Result<Schedule, StoreError> {
        if let Some(key) = &schedule.deduplication_key {
            if let Some(row) = sqlx::query("SELECT * FROM schedules WHERE deduplication_key = ?")
                .bind(key)
                .fetch_optional(self.pool())
                .await?
            {
                let mut existing = schedule_from_row(&row)?;
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
        let row = sqlx::query("SELECT * FROM schedules WHERE id = ?")
            .bind(id)
            .fetch_optional(self.pool())
            .await?;
        match row {
            Some(r) => Ok(Some(schedule_from_row(&r)?)),
            None => Ok(None),
        }
    }

    async fn list_schedules(&self) -> Result<Vec<Schedule>, StoreError> {
        let rows = sqlx::query("SELECT * FROM schedules ORDER BY created_at DESC")
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(schedule_from_row).collect()
    }

    async fn update_schedule(&self, schedule: &Schedule) -> Result<(), StoreError> {
        let n = sqlx::query(
            "UPDATE schedules SET task_id = ?, cron = ?, timezone = ?, external_id = ?,
             deduplication_key = ?, payload = ?, enabled = ?, next_fire_at = ?,
             updated_at = ? WHERE id = ?",
        )
        .bind(&schedule.task_id)
        .bind(&schedule.cron)
        .bind(&schedule.timezone)
        .bind(&schedule.external_id)
        .bind(&schedule.deduplication_key)
        .bind(serde_json::to_string(&schedule.payload)?)
        .bind(i64::from(schedule.enabled))
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
        let n = sqlx::query("DELETE FROM schedules WHERE id = ?")
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(n.rows_affected() == 1)
    }

    async fn migrate(&self) -> Result<(), StoreError> {
        sqlx::query(SCHEMA).execute(self.pool()).await?;
        // Upgrade path for databases created by v0.1.0 (runs table
        // without the Trigger-core columns): add any missing column.
        let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('runs')")
            .fetch_all(self.pool())
            .await?;
        let wanted: &[(&str, &str)] = &[
            ("concurrency_key", "TEXT"),
            ("tags", "TEXT NOT NULL DEFAULT '[]'"),
            ("metadata", "TEXT NOT NULL DEFAULT '{}'"),
            ("parent_run_id", "TEXT"),
            ("parent_wait_key", "TEXT"),
            ("schedule_id", "TEXT"),
            ("ttl_at", "INTEGER"),
            ("source_run_id", "TEXT"),
            ("leased_by", "TEXT"),
            ("lease_expires_at", "INTEGER"),
        ];
        for (name, ddl) in wanted {
            if !cols.iter().any(|c| c == name) {
                sqlx::query(&format!("ALTER TABLE runs ADD COLUMN {name} {ddl}"))
                    .execute(self.pool())
                    .await?;
            }
        }
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
        sqlx::query(
            "INSERT INTO runs (id, workflow, payload, status, attempt, max_attempts,
             next_attempt_at, cancel_requested, concurrency_key, tags, metadata,
             parent_run_id, parent_wait_key, schedule_id, ttl_at, source_run_id,
             created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(run.id.to_string())
        .bind(&run.workflow)
        .bind(serde_json::to_string(&run.payload)?)
        .bind(status_str(run.status))
        .bind(run.attempt as i64)
        .bind(run.max_attempts as i64)
        .bind(run.next_attempt_at.map(ms))
        .bind(&run.concurrency_key)
        .bind(serde_json::to_string(&run.tags)?)
        .bind(serde_json::to_string(&run.metadata)?)
        .bind(run.parent_run_id.map(|u| u.to_string()))
        .bind(&run.parent_wait_key)
        .bind(&run.schedule_id)
        .bind(run.ttl_at.map(ms))
        .bind(run.source_run_id.map(|u| u.to_string()))
        .bind(ms(now))
        .bind(ms(now))
        .execute(self.pool())
        .await?;
        Ok(run)
    }

    async fn get_run(&self, id: RunId) -> Result<Option<Run>, StoreError> {
        let row = sqlx::query("SELECT * FROM runs WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(self.pool())
            .await?;
        match row {
            Some(r) => Ok(Some(run_from_row(&r)?)),
            None => Ok(None),
        }
    }

    async fn list_runs(&self) -> Result<Vec<Run>, StoreError> {
        let rows = sqlx::query("SELECT * FROM runs ORDER BY created_at DESC")
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(run_from_row).collect()
    }

    async fn claim_due(&self, now_ms: i64) -> Result<Option<Run>, StoreError> {
        let mut tx = self.pool().begin().await?;
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
        sqlx::query("UPDATE runs SET status = 'Running', attempt = ?, leased_by = NULL, lease_expires_at = NULL, updated_at = ? WHERE id = ?")
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
             wait_token = ?, cancel_requested = ?, concurrency_key = ?,
             tags = ?, metadata = ?, ttl_at = ?, leased_by = ?,
             lease_expires_at = ?, updated_at = ? WHERE id = ?",
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
        .bind(&run.concurrency_key)
        .bind(serde_json::to_string(&run.tags)?)
        .bind(serde_json::to_string(&run.metadata)?)
        .bind(run.ttl_at.map(ms))
        .bind(&run.leased_by)
        .bind(run.lease_expires_at.map(ms))
        .bind(ms(run.updated_at))
        .bind(run.id.to_string())
        .execute(self.pool())
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
        .execute(self.pool())
        .await?;
        Ok(n.rows_affected() == 1)
    }

    async fn get_step(&self, run_id: RunId, key: &str) -> Result<Option<StepRecord>, StoreError> {
        let row = sqlx::query("SELECT * FROM steps WHERE run_id = ? AND key = ?")
            .bind(run_id.to_string())
            .bind(key)
            .fetch_optional(self.pool())
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
            .fetch_all(self.pool())
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
        let mut tx = self.pool().begin().await?;
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
            .fetch_all(self.pool())
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
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn get_wait(&self, run_id: RunId, key: &str) -> Result<Option<WaitRecord>, StoreError> {
        let row = sqlx::query("SELECT * FROM waits WHERE run_id = ? AND key = ?")
            .bind(run_id.to_string())
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
        sqlx::query("DELETE FROM waits WHERE run_id = ? AND key = ?")
            .bind(run_id.to_string())
            .bind(key)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn get_approval(&self, token: &str) -> Result<Option<serde_json::Value>, StoreError> {
        let row = sqlx::query("SELECT result FROM approvals WHERE token = ?")
            .bind(token)
            .fetch_optional(self.pool())
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
        let mut tx = self.pool().begin().await?;
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
                    .execute(self.pool())
                    .await?;
            }
            RunStatus::Running => {
                sqlx::query("UPDATE runs SET cancel_requested = 1, updated_at = ? WHERE id = ?")
                    .bind(ms(Utc::now()))
                    .bind(id.to_string())
                    .execute(self.pool())
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
    use ultraflows_core::RetryPolicy;
    use ultraflows_core::types::NewRun;

    async fn store() -> SqliteStore {
        let s = SqliteStore::connect_memory().await.unwrap();
        s.migrate().await.unwrap();
        s
    }

    #[tokio::test]
    async fn idempotent_trigger_returns_same_run_on_second_call() {
        let s = store().await;
        let now = Utc::now().timestamp_millis();
        let (run1, created1) = s
            .create_run_idempotent(
                NewRun::new("task-a", serde_json::json!({"x": 1}), 3),
                IdempotencyScope::Run,
                "key-1",
                now + 86_400_000,
            )
            .await
            .unwrap();
        let (run2, created2) = s
            .create_run_idempotent(
                NewRun::new("task-a", serde_json::json!({"x": 999}), 3),
                IdempotencyScope::Run,
                "key-1",
                now + 86_400_000,
            )
            .await
            .unwrap();
        assert!(created1 && !created2);
        assert_eq!(run1.id, run2.id);
        assert_eq!(run2.payload["x"], 1, "first payload wins");
        // Global scope: same key on a different task still dedupes
        // (against a Global-scoped key — scopes are separate namespaces).
        let (g1, g_created) = s
            .create_run_idempotent(
                NewRun::new("task-b", serde_json::json!({}), 3),
                IdempotencyScope::Global,
                "gkey-1",
                now + 86_400_000,
            )
            .await
            .unwrap();
        assert!(g_created);
        let (run3, created3) = s
            .create_run_idempotent(
                NewRun::new("task-a", serde_json::json!({}), 3),
                IdempotencyScope::Global,
                "gkey-1",
                now + 86_400_000,
            )
            .await
            .unwrap();
        assert!(!created3);
        assert_eq!(run3.id, g1.id);
        // Different scope key on same task: a new run.
        let (_, created4) = s
            .create_run_idempotent(
                NewRun::new("task-a", serde_json::json!({}), 3),
                IdempotencyScope::Run,
                "key-2",
                now + 86_400_000,
            )
            .await
            .unwrap();
        assert!(created4);
    }

    #[tokio::test]
    async fn ttl_expires_only_never_started_runs() {
        let s = store().await;
        let now = Utc::now();
        let mut input = NewRun::new("task-a", serde_json::json!({}), 3);
        input.ttl_at = Some(now - chrono::Duration::seconds(1));
        let run = s.create_run(input).await.unwrap();
        let expired = s.expire_ttl(now.timestamp_millis()).await.unwrap();
        assert_eq!(expired, vec![run.id]);
        let after = s.get_run(run.id).await.unwrap().unwrap();
        assert_eq!(after.status, RunStatus::Expired);
        assert!(after.status.is_terminal());
    }

    #[tokio::test]
    async fn schedule_dedup_upsert_keeps_one_schedule() {
        let s = store().await;
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
    }

    #[tokio::test]
    async fn child_completion_wakes_parent() {
        let s = store().await;
        let child = s
            .create_run(NewRun::new("child-task", serde_json::json!({}), 3))
            .await
            .unwrap();
        let parent = s
            .create_run(NewRun::new("parent-task", serde_json::json!({}), 3))
            .await
            .unwrap();
        // Simulate a parent suspended on a child wait.
        let mut p = s.get_run(parent.id).await.unwrap().unwrap();
        p.status = RunStatus::Waiting;
        p.wait_kind = Some("child".to_string());
        p.wait_token = Some(child.id.to_string());
        s.update_run(&p).await.unwrap();
        let woken = s
            .wake_child_parents(child.id, Utc::now().timestamp_millis())
            .await
            .unwrap();
        assert_eq!(woken, vec![parent.id]);
        let after = s.get_run(parent.id).await.unwrap().unwrap();
        assert_eq!(after.status, RunStatus::Pending);
    }

    #[tokio::test]
    async fn lease_claim_and_expiry() {
        let s = store().await;
        let run = s
            .create_run(NewRun::new("task-a", serde_json::json!({}), 3))
            .await
            .unwrap();
        let now = Utc::now().timestamp_millis();
        let claimed = s
            .claim_specific(run.id, "worker-1", now)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claimed.attempt, 1);
        assert_eq!(claimed.leased_by.as_deref(), Some("worker-1"));
        // Still leased: second claim fails.
        assert!(
            s.claim_specific(run.id, "worker-2", now)
                .await
                .unwrap()
                .is_none()
        );
        // After lease expiry (30s): claimable again, attempt increments.
        let claimed2 = s
            .claim_specific(run.id, "worker-2", now + 31_000)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claimed2.attempt, 2);
        assert_eq!(claimed2.leased_by.as_deref(), Some("worker-2"));
    }

    #[tokio::test]
    async fn task_registry_roundtrip() {
        let s = store().await;
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
        let got = s.get_task("task-a").await.unwrap().unwrap();
        assert_eq!(got.concurrency_total, Some(3));
        assert_eq!(got.retry.max_attempts, 5);
        assert_eq!(s.list_tasks().await.unwrap().len(), 1);
    }
}
