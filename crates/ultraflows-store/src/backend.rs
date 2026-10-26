//! Backend selection: one concrete type the server can hold while the
//! `Store` trait itself stays non-object-safe (async fn in trait).
//! `AnyStore` delegates every trait method to the SQLite or the
//! PostgreSQL backend. Adding a third backend means one new variant
//! plus one arm per method here — the server code does not change.

use crate::error::StoreError;
use crate::postgres::PostgresStore;
use crate::sqlite::SqliteStore;
use crate::traits::Store;
use ultraflows_core::types::{
    IdempotencyScope, NewRun, Run, RunEvent, RunId, Schedule, StepRecord, TaskDefinition,
    WaitRecord,
};
use uuid::Uuid;

/// Either storage backend, chosen at server startup from the database
/// URL (see `ultraflows-server` main and docs/POSTGRES.md).
#[derive(Debug, Clone)]
pub enum AnyStore {
    Sqlite(SqliteStore),
    Postgres(PostgresStore),
}

impl AnyStore {
    pub fn sqlite(store: SqliteStore) -> Self {
        Self::Sqlite(store)
    }

    pub fn postgres(store: PostgresStore) -> Self {
        Self::Postgres(store)
    }
}

/// Delegate a by-reference / Copy-argument method to the active variant.
/// Owned-argument methods (create_run, create_run_idempotent,
/// append_event, record_approval) are implemented by hand below because
/// moving the same owned value into both match arms does not compile.
macro_rules! dispatch {
    ($self:ident, $method:ident $(, $arg:expr)*) => {
        match $self {
            AnyStore::Sqlite(s) => s.$method($($arg),*).await,
            AnyStore::Postgres(s) => s.$method($($arg),*).await,
        }
    };
}

impl Store for AnyStore {
    async fn migrate(&self) -> Result<(), StoreError> {
        dispatch!(self, migrate)
    }

    async fn create_run(&self, input: NewRun) -> Result<Run, StoreError> {
        match self {
            AnyStore::Sqlite(s) => s.create_run(input.clone()).await,
            AnyStore::Postgres(s) => s.create_run(input).await,
        }
    }

    async fn get_run(&self, id: RunId) -> Result<Option<Run>, StoreError> {
        dispatch!(self, get_run, id)
    }

    async fn list_runs(&self) -> Result<Vec<Run>, StoreError> {
        dispatch!(self, list_runs)
    }

    async fn claim_due(&self, now_ms: i64) -> Result<Option<Run>, StoreError> {
        dispatch!(self, claim_due, now_ms)
    }

    async fn update_run(&self, run: &Run) -> Result<(), StoreError> {
        dispatch!(self, update_run, run)
    }

    async fn record_step(&self, record: &StepRecord) -> Result<bool, StoreError> {
        dispatch!(self, record_step, record)
    }

    async fn get_step(&self, run_id: RunId, key: &str) -> Result<Option<StepRecord>, StoreError> {
        dispatch!(self, get_step, run_id, key)
    }

    async fn list_steps(&self, run_id: RunId) -> Result<Vec<StepRecord>, StoreError> {
        dispatch!(self, list_steps, run_id)
    }

    async fn append_event(
        &self,
        run_id: RunId,
        kind: &str,
        payload: serde_json::Value,
    ) -> Result<RunEvent, StoreError> {
        match self {
            AnyStore::Sqlite(s) => s.append_event(run_id, kind, payload.clone()).await,
            AnyStore::Postgres(s) => s.append_event(run_id, kind, payload).await,
        }
    }

    async fn list_events(&self, run_id: RunId) -> Result<Vec<RunEvent>, StoreError> {
        dispatch!(self, list_events, run_id)
    }

    async fn upsert_wait(&self, wait: &WaitRecord) -> Result<(), StoreError> {
        dispatch!(self, upsert_wait, wait)
    }

    async fn get_wait(&self, run_id: RunId, key: &str) -> Result<Option<WaitRecord>, StoreError> {
        dispatch!(self, get_wait, run_id, key)
    }

    async fn resolve_wait(&self, run_id: RunId, key: &str) -> Result<(), StoreError> {
        dispatch!(self, resolve_wait, run_id, key)
    }

    async fn get_approval(&self, token: &str) -> Result<Option<serde_json::Value>, StoreError> {
        dispatch!(self, get_approval, token)
    }

    async fn record_approval(
        &self,
        token: &str,
        result: serde_json::Value,
        now_ms: i64,
    ) -> Result<Vec<Uuid>, StoreError> {
        match self {
            AnyStore::Sqlite(s) => s.record_approval(token, result.clone(), now_ms).await,
            AnyStore::Postgres(s) => s.record_approval(token, result, now_ms).await,
        }
    }

    async fn request_cancel(&self, id: RunId) -> Result<bool, StoreError> {
        dispatch!(self, request_cancel, id)
    }

    async fn upsert_task(&self, task: &TaskDefinition) -> Result<(), StoreError> {
        dispatch!(self, upsert_task, task)
    }

    async fn get_task(&self, id: &str) -> Result<Option<TaskDefinition>, StoreError> {
        dispatch!(self, get_task, id)
    }

    async fn list_tasks(&self) -> Result<Vec<TaskDefinition>, StoreError> {
        dispatch!(self, list_tasks)
    }

    async fn create_run_idempotent(
        &self,
        input: NewRun,
        scope: IdempotencyScope,
        key: &str,
        expires_at_ms: i64,
    ) -> Result<(Run, bool), StoreError> {
        match self {
            AnyStore::Sqlite(s) => {
                s.create_run_idempotent(input.clone(), scope, key, expires_at_ms)
                    .await
            }
            AnyStore::Postgres(s) => {
                s.create_run_idempotent(input, scope, key, expires_at_ms)
                    .await
            }
        }
    }

    async fn list_runs_filtered(
        &self,
        status: Option<&str>,
        task: Option<&str>,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
    ) -> Result<Vec<Run>, StoreError> {
        dispatch!(self, list_runs_filtered, status, task, from_ms, to_ms)
    }

    async fn list_due(&self, now_ms: i64) -> Result<Vec<Run>, StoreError> {
        dispatch!(self, list_due, now_ms)
    }

    async fn claim_specific(
        &self,
        run_id: RunId,
        worker_id: &str,
        now_ms: i64,
    ) -> Result<Option<Run>, StoreError> {
        dispatch!(self, claim_specific, run_id, worker_id, now_ms)
    }

    async fn count_running_total(&self, task_id: &str) -> Result<u32, StoreError> {
        dispatch!(self, count_running_total, task_id)
    }

    async fn count_running_for_key(&self, task_id: &str, key: &str) -> Result<u32, StoreError> {
        dispatch!(self, count_running_for_key, task_id, key)
    }

    async fn expire_ttl(&self, now_ms: i64) -> Result<Vec<Uuid>, StoreError> {
        dispatch!(self, expire_ttl, now_ms)
    }

    async fn promote_pending_version(&self, task_id: &str) -> Result<Vec<Uuid>, StoreError> {
        dispatch!(self, promote_pending_version, task_id)
    }

    async fn suspend_run(&self, run_id: RunId) -> Result<bool, StoreError> {
        dispatch!(self, suspend_run, run_id)
    }

    async fn wake_child_parents(
        &self,
        child_id: RunId,
        now_ms: i64,
    ) -> Result<Vec<Uuid>, StoreError> {
        dispatch!(self, wake_child_parents, child_id, now_ms)
    }

    async fn touch_worker(&self, worker_id: &str, now_ms: i64) -> Result<(), StoreError> {
        dispatch!(self, touch_worker, worker_id, now_ms)
    }

    async fn create_schedule(&self, schedule: &Schedule) -> Result<(), StoreError> {
        dispatch!(self, create_schedule, schedule)
    }

    async fn upsert_schedule_by_dedup(&self, schedule: &Schedule) -> Result<Schedule, StoreError> {
        dispatch!(self, upsert_schedule_by_dedup, schedule)
    }

    async fn get_schedule(&self, id: &str) -> Result<Option<Schedule>, StoreError> {
        dispatch!(self, get_schedule, id)
    }

    async fn list_schedules(&self) -> Result<Vec<Schedule>, StoreError> {
        dispatch!(self, list_schedules)
    }

    async fn update_schedule(&self, schedule: &Schedule) -> Result<(), StoreError> {
        dispatch!(self, update_schedule, schedule)
    }

    async fn delete_schedule(&self, id: &str) -> Result<bool, StoreError> {
        dispatch!(self, delete_schedule, id)
    }
}
