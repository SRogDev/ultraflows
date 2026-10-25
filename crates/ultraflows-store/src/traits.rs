use crate::error::StoreError;
use ultraflows_core::types::{
    IdempotencyScope, NewRun, Run, RunEvent, RunId, Schedule, StepRecord, TaskDefinition,
    WaitRecord,
};
use uuid::Uuid;

/// The storage contract (ADR-0003). The runtime's correctness rules may
/// only depend on this trait, never on a concrete database.
///
/// v0.1.0 implements this trait for SQLite. A PostgreSQL implementation
/// must pass the same conformance behavior (insert-once steps, gapless
/// per-run event sequences, atomic claims) before it is called "real".
#[allow(async_fn_in_trait)]
pub trait Store: Send + Sync + Clone + 'static {
    /// Create schema if absent. Idempotent.
    async fn migrate(&self) -> Result<(), StoreError>;
    async fn create_run(&self, input: NewRun) -> Result<Run, StoreError>;
    async fn get_run(&self, id: RunId) -> Result<Option<Run>, StoreError>;
    async fn list_runs(&self) -> Result<Vec<Run>, StoreError>;
    /// Atomically claim one due run (Pending retry-due, or Waiting timer
    /// due) and mark it Running with attempt incremented. Returns None
    /// when nothing is due.
    async fn claim_due(&self, now_ms: i64) -> Result<Option<Run>, StoreError>;
    /// Persist a run's mutable fields after the runtime validated the
    /// state transition in `ultraflows-core`.
    async fn update_run(&self, run: &Run) -> Result<(), StoreError>;
    /// Record a step result. Returns true when this call inserted it,
    /// false when a result for (run_id, key) already existed (I2: the
    /// first result wins, always).
    async fn record_step(&self, record: &StepRecord) -> Result<bool, StoreError>;
    async fn get_step(&self, run_id: RunId, key: &str) -> Result<Option<StepRecord>, StoreError>;
    async fn list_steps(&self, run_id: RunId) -> Result<Vec<StepRecord>, StoreError>;
    async fn append_event(
        &self,
        run_id: RunId,
        kind: &str,
        payload: serde_json::Value,
    ) -> Result<RunEvent, StoreError>;
    async fn list_events(&self, run_id: RunId) -> Result<Vec<RunEvent>, StoreError>;
    async fn upsert_wait(&self, wait: &WaitRecord) -> Result<(), StoreError>;
    async fn get_wait(&self, run_id: RunId, key: &str) -> Result<Option<WaitRecord>, StoreError>;
    async fn resolve_wait(&self, run_id: RunId, key: &str) -> Result<(), StoreError>;
    async fn get_approval(&self, token: &str) -> Result<Option<serde_json::Value>, StoreError>;
    /// Record an approval and wake every Waiting run suspended on it
    /// (status -> Pending). Returns the woken run ids.
    async fn record_approval(
        &self,
        token: &str,
        result: serde_json::Value,
        now_ms: i64,
    ) -> Result<Vec<Uuid>, StoreError>;
    /// Cancel: Waiting/Pending runs become Cancelled immediately;
    /// Running runs get `cancel_requested` set (observed at the next
    /// step boundary). Returns true when the run existed.
    async fn request_cancel(&self, id: RunId) -> Result<bool, StoreError>;

    // ---- Task registry ----
    async fn upsert_task(&self, task: &TaskDefinition) -> Result<(), StoreError>;
    async fn get_task(&self, id: &str) -> Result<Option<TaskDefinition>, StoreError>;
    async fn list_tasks(&self) -> Result<Vec<TaskDefinition>, StoreError>;

    // ---- Idempotent trigger ----
    /// Create a run under an idempotency key, atomically. Returns the
    /// run and whether this call created it (false = the key already
    /// existed and the original run is returned).
    async fn create_run_idempotent(
        &self,
        input: NewRun,
        scope: IdempotencyScope,
        key: &str,
        expires_at_ms: i64,
    ) -> Result<(Run, bool), StoreError>;

    // ---- Filtered listing / leasing ----
    async fn list_runs_filtered(
        &self,
        status: Option<&str>,
        task: Option<&str>,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
    ) -> Result<Vec<Run>, StoreError>;
    /// Due runs (Pending claimable, or Waiting timer due), oldest first.
    async fn list_due(&self, now_ms: i64) -> Result<Vec<Run>, StoreError>;
    /// Atomically claim one specific run for a remote worker (lease).
    /// Returns the claimed run (with attempt incremented and lease set),
    /// or None if the run was no longer claimable.
    async fn claim_specific(
        &self,
        run_id: RunId,
        worker_id: &str,
        now_ms: i64,
    ) -> Result<Option<Run>, StoreError>;
    async fn count_running_total(&self, task_id: &str) -> Result<u32, StoreError>;
    async fn count_running_for_key(&self, task_id: &str, key: &str) -> Result<u32, StoreError>;
    /// Expire Pending, never-started runs past their TTL. Returns ids.
    async fn expire_ttl(&self, now_ms: i64) -> Result<Vec<Uuid>, StoreError>;
    /// Promote PendingVersion runs of a newly registered task to Pending.
    async fn promote_pending_version(&self, task_id: &str) -> Result<Vec<Uuid>, StoreError>;
    /// Mark a Running run as Waiting (remote worker suspending on a wait).
    async fn suspend_run(&self, run_id: RunId) -> Result<bool, StoreError>;
    /// Wake parents waiting on a child run (triggerAndWait). Returns ids.
    async fn wake_child_parents(
        &self,
        child_id: RunId,
        now_ms: i64,
    ) -> Result<Vec<Uuid>, StoreError>;

    /// Record a worker heartbeat (upsert by worker id).
    async fn touch_worker(&self, worker_id: &str, now_ms: i64) -> Result<(), StoreError>;

    // ---- Schedules ----
    async fn create_schedule(&self, schedule: &Schedule) -> Result<(), StoreError>;
    async fn upsert_schedule_by_dedup(&self, schedule: &Schedule) -> Result<Schedule, StoreError>;
    async fn get_schedule(&self, id: &str) -> Result<Option<Schedule>, StoreError>;
    async fn list_schedules(&self) -> Result<Vec<Schedule>, StoreError>;
    async fn update_schedule(&self, schedule: &Schedule) -> Result<(), StoreError>;
    async fn delete_schedule(&self, id: &str) -> Result<bool, StoreError>;
}
