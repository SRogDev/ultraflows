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
    /// Claimed by a worker and executing.
    Running,
    /// Suspended on a wait (timer or approval). Costs no worker slot.
    Waiting,
    Completed,
    Failed,
    Cancelled,
}

impl RunStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled
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
}

/// Deterministic idempotency key for a step, per the honesty contract:
/// an external system that accepts this key can neutralize a duplicate
/// execution of the step's effect.
pub fn idempotency_key(run_id: RunId, step_key: &str) -> String {
    format!("uf_{run_id}_{step_key}")
}
