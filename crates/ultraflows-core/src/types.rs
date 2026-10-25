use crate::retry::RetryPolicy;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The durable identity of a run. Time-ordered (v7) so listing by id is
/// roughly chronological.
pub type RunId = Uuid;

/// Lifecycle of a run. Terminal states never execute another step (I3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RunStatus {
    /// Created, ready to be claimed (includes scheduled retries).
    Pending,
    /// Created for a task that is not (yet) registered. Visible, but
    /// never executed: when the task registers, the run promotes to
    /// Pending; it never silently runs against the wrong code.
    PendingVersion,
    /// Claimed by a worker and executing.
    Running,
    /// Suspended on a wait (timer or approval). Costs no worker slot.
    Waiting,
    Completed,
    Failed,
    Cancelled,
    /// Expired by TTL before its first attempt started.
    Expired,
}

impl RunStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled | RunStatus::Expired
        )
    }
}

/// One execution of a workflow definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: RunId,
    pub workflow: String,
    pub payload: serde_json::Value,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub status: RunStatus,
    /// Attempts already started (1-based while running).
    pub attempt: u32,
    pub max_attempts: u32,
    /// When a Pending run may be claimed again (retry schedule).
    pub next_attempt_at: Option<DateTime<Utc>>,
    /// When a Waiting timer run may be claimed again.
    pub wake_at: Option<DateTime<Utc>>,
    /// Key of the wait the run is suspended on, if any.
    pub wait_key: Option<String>,
    pub wait_kind: Option<String>,
    /// Approval token of the wait, if any (surfaced so an API caller can
    /// complete it — the token is not a secret in this demo).
    pub wait_token: Option<String>,
    /// Set by a cancel request while the run is Running; the next step
    /// boundary observes it and ends the run as Cancelled.
    pub cancel_requested: bool,
    /// Concurrency key (Trigger's `concurrencyKey`): runs sharing a
    /// task + key share a per-key concurrency pool.
    pub concurrency_key: Option<String>,
    /// Free-form tags for filtering (Trigger's `tags`).
    pub tags: Vec<String>,
    /// Free-form metadata (Trigger's `metadata`, mutable by design).
    pub metadata: serde_json::Value,
    /// For triggerAndWait children: the parent run and the parent's
    /// wait key that this child resolves.
    pub parent_run_id: Option<RunId>,
    pub parent_wait_key: Option<String>,
    /// Schedule that created this run, if any.
    pub schedule_id: Option<String>,
    /// TTL deadline (from enqueue). A Pending run that has not started
    /// by this time becomes Expired instead of executing.
    pub ttl_at: Option<DateTime<Utc>>,
    /// Run this run was replayed from, if any.
    pub source_run_id: Option<RunId>,
    /// Worker currently leasing this run (remote-worker protocol).
    pub leased_by: Option<String>,
    /// Lease expiry for remote workers; a Running run whose lease has
    /// expired may be reclaimed by another worker.
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A recorded step result. `(run_id, key)` is unique forever (I2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRecord {
    pub run_id: RunId,
    pub key: String,
    pub result: serde_json::Value,
    /// Deterministic idempotency key derived from `(run_id, key)`.
    pub idempotency_key: String,
}

/// One entry of the append-only run event log (I6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEvent {
    pub run_id: RunId,
    pub seq: i64,
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// A persisted wait: a timer, or an approval (token) wait.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitRecord {
    pub run_id: RunId,
    pub key: String,
    pub kind: String, // "timer" | "approval"
    pub token: Option<String>,
    pub wake_at: Option<DateTime<Utc>>,
}

/// Input for creating a run.
#[derive(Debug, Clone)]
pub struct NewRun {
    pub workflow: String,
    pub payload: serde_json::Value,
    pub max_attempts: u32,
    pub concurrency_key: Option<String>,
    pub tags: Vec<String>,
    pub metadata: serde_json::Value,
    pub parent_run_id: Option<RunId>,
    pub parent_wait_key: Option<String>,
    pub schedule_id: Option<String>,
    /// TTL deadline (absolute). None = no TTL.
    pub ttl_at: Option<DateTime<Utc>>,
    pub source_run_id: Option<RunId>,
    /// Earliest claim time (Trigger's `delay`): the run is Pending but
    /// not claimable before this instant.
    pub next_attempt_at: Option<DateTime<Utc>>,
}

impl NewRun {
    pub fn new(workflow: impl Into<String>, payload: serde_json::Value, max_attempts: u32) -> Self {
        Self {
            workflow: workflow.into(),
            payload,
            max_attempts,
            concurrency_key: None,
            tags: Vec::new(),
            metadata: serde_json::json!({}),
            parent_run_id: None,
            parent_wait_key: None,
            schedule_id: None,
            ttl_at: None,
            source_run_id: None,
            next_attempt_at: None,
        }
    }
}

/// A registered task definition (Trigger-shaped `task()`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDefinition {
    pub id: String,
    /// Hash of the definition as registered (definition drift detection:
    /// a run pins the hash it was created against; a re-registration
    /// with a different hash updates the definition, and runs created
    /// afterwards use the new hash).
    pub version_hash: String,
    pub retry: RetryPolicy,
    /// Total concurrent Running runs for this task (Trigger's
    /// `concurrency.total`). None = unlimited.
    pub concurrency_total: Option<u32>,
    /// Concurrent Running runs per concurrency key (Trigger's
    /// `concurrency.perKey`). None = unlimited.
    pub concurrency_per_key: Option<u32>,
    pub queue: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// An imperative schedule (Trigger's `schedules.create()`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub id: String,
    pub task_id: String,
    /// 5-field cron pattern (no seconds). `L` is rejected (see schedule.rs).
    pub cron: String,
    pub timezone: String,
    pub external_id: Option<String>,
    /// Upsert key: creating a schedule with an existing
    /// deduplication_key updates it instead of creating a duplicate.
    pub deduplication_key: Option<String>,
    pub payload: serde_json::Value,
    pub enabled: bool,
    pub next_fire_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Idempotency-key scope (Trigger's idempotency model, trigger-endpoint
/// form): `run` scopes a key to one task, `global` scopes it to nothing
/// (one key, one run, everywhere), `attempt` is treated like `run` for
/// top-level triggers (there is no attempt yet at trigger time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdempotencyScope {
    #[default]
    Run,
    Attempt,
    Global,
}

impl IdempotencyScope {
    /// Storage key for a trigger idempotency key.
    pub fn storage_key(&self, task_id: &str, key: &str) -> (String, String) {
        match self {
            IdempotencyScope::Global => ("global".to_string(), key.to_string()),
            _ => ("run".to_string(), format!("{task_id}:{key}")),
        }
    }
}

/// Deterministic idempotency key for a step, per the honesty contract:
/// an external system that accepts this key can neutralize a duplicate
/// execution of the step's effect.
pub fn idempotency_key(run_id: RunId, step_key: &str) -> String {
    format!("uf_{run_id}_{step_key}")
}
