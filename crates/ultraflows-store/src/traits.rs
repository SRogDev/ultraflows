use crate::error::StoreError;
use ultraflows_core::types::{NewRun, Run, RunEvent, RunId, StepRecord, WaitRecord};
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
}
