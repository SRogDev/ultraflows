//! HTTP API — Trigger-shaped, own protocol (NOT wire-compatible with
//! Trigger.dev's hosted API).
//!
//! Trigger API:
//! - POST /v1/tasks/{id}/trigger — trigger a run (delay, ttl,
//!   idempotencyKey + scope, concurrencyKey, maxAttempts, tags, metadata)
//! - POST /v1/tasks/{id}/batch-trigger — up to 100 runs in one call
//! - GET /v1/runs[?status&task&tag&from&to] — filtered run list
//! - GET /v1/runs/{id}, /events, /steps, /waits/{key}
//! - POST /v1/runs/{id}/cancel, /replay, /reschedule (delayed only)
//! - POST /v1/schedules, PATCH /v1/schedules/{id}, DELETE /v1/schedules/{id}
//! - POST /v1/waitpoints/{token}/complete (alias: /v1/approvals/{token})
//! - GET /v1/waitpoints/{token} — stored waitpoint result, if any
//!
//! Worker protocol (remote workers lease work; the server never runs
//! task code itself):
//! - POST /v1/workers/register — upsert task definitions, promote
//!   PendingVersion runs of newly registered tasks
//! - POST /v1/workers/{id}/heartbeat
//! - POST /v1/workers/{id}/lease — claim one due run (concurrency caps
//!   enforced here; blocked runs are skipped, not head-of-line blocking)
//! - POST /v1/workers/leases/{run_id}/step — report a step result
//!   (insert-once; a duplicate report returns the recorded result)
//! - POST /v1/workers/leases/{run_id}/wait — suspend on a wait
//!   (timer / approval token / child run)
//! - POST /v1/workers/leases/{run_id}/complete, /fail, /heartbeat
//!
//! No authentication in this release: bind localhost-only. The README
//! says so. Do not expose this server to a network you do not trust.
//!
//! Idempotency TTL default: 30 days (an UltraFlows choice, taken from
//! the docs recon; the REA map notes Trigger's code paths read did not
//! establish a default — see docs/TRIGGER_CORE_MAP.md section 9.3),
//! unless the trigger call passes `idempotencyTtlMs`.

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, patch, post};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use ultraflows_core::types::{
    IdempotencyScope, NewRun, Run, RunId, RunStatus, Schedule, StepRecord, TaskDefinition,
    WaitRecord,
};
use ultraflows_core::{RetryPolicy, next_fire};
use ultraflows_store::{SqliteStore, Store};
use uuid::Uuid;

pub const LEASE_MS: i64 = 30_000;
pub const DEFAULT_IDEMPOTENCY_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1000;
pub const BATCH_MAX: usize = 100;

#[derive(Clone)]
pub struct AppState {
    pub store: SqliteStore,
}

type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;

fn not_found(id: &str) -> (StatusCode, String) {
    (StatusCode::NOT_FOUND, format!("not found: {id}"))
}

fn parse_run_id(id: &str) -> Result<RunId, (StatusCode, String)> {
    Uuid::parse_str(id).map_err(|_| (StatusCode::BAD_REQUEST, format!("invalid run id: {id}")))
}

// ---------- shared trigger logic (used by the API and the scheduler) ----------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TriggerOptions {
    pub payload: Option<serde_json::Value>,
    pub delay_ms: Option<u64>,
    pub ttl_ms: Option<u64>,
    pub idempotency_key: Option<String>,
    pub idempotency_scope: Option<IdempotencyScope>,
    pub idempotency_ttl_ms: Option<u64>,
    pub concurrency_key: Option<String>,
    pub max_attempts: Option<u32>,
    pub tags: Option<Vec<String>>,
    pub metadata: Option<serde_json::Value>,
}

/// Trigger one run. Returns the run and whether this call created it.
/// An unregistered task yields a PendingVersion run: visible, never
/// executed, promoted to Pending when the task registers.
pub async fn trigger_task(
    store: &SqliteStore,
    task_id: &str,
    opts: TriggerOptions,
    schedule_id: Option<String>,
) -> Result<(Run, bool), String> {
    let now = Utc::now();
    let task = store.get_task(task_id).await.map_err(|e| e.to_string())?;
    let max_attempts = opts
        .max_attempts
        .or_else(|| task.as_ref().map(|t| t.retry.max_attempts))
        .unwrap_or(5);
    let mut input = NewRun::new(
        task_id,
        opts.payload.unwrap_or(serde_json::json!({})),
        max_attempts,
    );
    input.next_attempt_at = opts
        .delay_ms
        .map(|d| now + chrono::Duration::milliseconds(d as i64));
    input.ttl_at = opts
        .ttl_ms
        .map(|t| now + chrono::Duration::milliseconds(t as i64));
    input.concurrency_key = opts.concurrency_key;
    input.tags = opts.tags.unwrap_or_default();
    input.metadata = opts.metadata.unwrap_or(serde_json::json!({}));
    input.schedule_id = schedule_id;
    let (run, created) = match (&opts.idempotency_key, opts.idempotency_scope) {
        (Some(key), scope) => {
            let ttl = opts
                .idempotency_ttl_ms
                .map(|v| v as i64)
                .unwrap_or_else(|| now.timestamp_millis() + DEFAULT_IDEMPOTENCY_TTL_MS);
            store
                .create_run_idempotent(input, scope.unwrap_or(IdempotencyScope::Run), key, ttl)
                .await
                .map_err(|e| e.to_string())?
        }
        (None, _) => (
            store.create_run(input).await.map_err(|e| e.to_string())?,
            true,
        ),
    };
    if created && task.is_none() {
        // Unregistered task: hold the run visibly instead of executing
        // it against wrong code (or failing the trigger silently).
        let mut r = run.clone();
        r.status = RunStatus::PendingVersion;
        r.updated_at = Utc::now();
        store.update_run(&r).await.map_err(|e| e.to_string())?;
        store
            .append_event(r.id, "PendingVersion", serde_json::json!({"task": task_id}))
            .await
            .map_err(|e| e.to_string())?;
        return Ok((r, true));
    }
    if created {
        store
            .append_event(run.id, "Created", serde_json::json!({"task": task_id}))
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok((run, created))
}

// ---------- request / response types ----------

#[derive(Debug, Deserialize)]
pub struct TaskInput {
    pub id: String,
    pub version_hash: String,
    pub retry: Option<RetryPolicy>,
    pub concurrency_total: Option<u32>,
    pub concurrency_per_key: Option<u32>,
    pub queue: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub worker_id: Option<String>,
    pub tasks: Vec<TaskInput>,
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub worker_id: String,
    pub tasks: Vec<String>,
    pub promoted_runs: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct LeaseRequest {
    /// Task ids this worker serves. Only runs of these tasks are leased.
    pub tasks: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct LeasedRun {
    pub run: Run,
    /// Journaled steps recorded so far (replay/memoization input).
    pub steps: Vec<StepRecord>,
}

#[derive(Debug, Deserialize)]
pub struct StepReport {
    pub key: String,
    pub result: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct StepReportResponse {
    pub recorded: bool,
    pub result: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct WaitReport {
    pub key: String,
    pub kind: String,
    pub wake_at: Option<DateTime<Utc>>,
    pub token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CompleteReport {
    pub output: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct FailReport {
    pub error: String,
    pub worker_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HeartbeatReport {
    pub worker_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ScheduleInput {
    pub task_id: String,
    pub cron: String,
    pub timezone: String,
    pub payload: Option<serde_json::Value>,
    pub external_id: Option<String>,
    pub deduplication_key: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct SchedulePatch {
    pub cron: Option<String>,
    pub timezone: Option<String>,
    pub payload: Option<serde_json::Value>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct WaitpointComplete {
    pub output: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ReplayRequest {
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct RescheduleRequest {
    pub delay_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct BatchItem {
    #[serde(flatten)]
    pub opts: TriggerOptions,
}

#[derive(Debug, Deserialize)]
pub struct BatchRequest {
    pub items: Vec<BatchItem>,
}

#[derive(Debug, Serialize)]
pub struct BatchItemResult {
    pub run_id: Uuid,
    pub status: RunStatus,
    pub created: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct RunFilters {
    pub status: Option<String>,
    pub task: Option<String>,
    pub tag: Option<String>,
    pub from: Option<i64>,
    pub to: Option<i64>,
}

// ---------- workers ----------

async fn register_worker(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterRequest>,
) -> ApiResult<RegisterResponse> {
    let worker_id = req.worker_id.unwrap_or_else(|| Uuid::now_v7().to_string());
    let now = Utc::now();
    let mut ids = Vec::new();
    let mut promoted = Vec::new();
    for t in req.tasks {
        let def = TaskDefinition {
            id: t.id.clone(),
            version_hash: t.version_hash,
            retry: t.retry.unwrap_or_default(),
            concurrency_total: t.concurrency_total,
            concurrency_per_key: t.concurrency_per_key,
            queue: t.queue,
            created_at: now,
            updated_at: now,
        };
        state
            .store
            .upsert_task(&def)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let p = state
            .store
            .promote_pending_version(&t.id)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        promoted.extend(p);
        ids.push(t.id);
    }
    state
        .store
        .touch_worker(&worker_id, now.timestamp_millis())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(RegisterResponse {
        worker_id,
        tasks: ids,
        promoted_runs: promoted,
    }))
}

async fn worker_heartbeat(
    State(state): State<Arc<AppState>>,
    Path(worker_id): Path<String>,
) -> ApiResult<serde_json::Value> {
    state
        .store
        .touch_worker(&worker_id, Utc::now().timestamp_millis())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn lease_work(
    State(state): State<Arc<AppState>>,
    Path(worker_id): Path<String>,
    Json(req): Json<LeaseRequest>,
) -> ApiResult<Option<LeasedRun>> {
    let now = Utc::now().timestamp_millis();
    // Candidates: due Pending/Waiting-timer runs + lease-expired Running
    // runs, oldest first. Blocked runs are skipped, never head-of-line
    // blocking the queue behind a capped task.
    let mut candidates = state
        .store
        .list_due(now)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let running = state
        .store
        .list_runs_filtered(Some("Running"), None, None, None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    for r in running {
        if r.lease_expires_at
            .map(|l| l.timestamp_millis() <= now)
            .unwrap_or(false)
        {
            candidates.push(r);
        }
    }
    candidates.sort_by_key(|r| r.created_at);
    for run in candidates {
        if !req.tasks.contains(&run.workflow) {
            continue;
        }
        // Concurrency caps, enforced at claim time.
        if let Some(task) = state
            .store
            .get_task(&run.workflow)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        {
            let total = state
                .store
                .count_running_total(&run.workflow)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            let already_running = run.status == RunStatus::Running;
            if let Some(cap) = task.concurrency_total {
                if !already_running && total >= cap {
                    continue;
                }
                if already_running && total > cap {
                    continue;
                }
            }
            if let (Some(cap), Some(key)) = (task.concurrency_per_key, &run.concurrency_key) {
                let per_key = state
                    .store
                    .count_running_for_key(&run.workflow, key)
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                if !already_running && per_key >= cap {
                    continue;
                }
                if already_running && per_key > cap {
                    continue;
                }
            }
        }
        if let Some(claimed) = state
            .store
            .claim_specific(run.id, &worker_id, now)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        {
            let steps = state
                .store
                .list_steps(claimed.id)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            state
                .store
                .append_event(
                    claimed.id,
                    "AttemptStarted",
                    serde_json::json!({"attempt": claimed.attempt, "worker": worker_id}),
                )
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            return Ok(Json(Some(LeasedRun {
                run: claimed,
                steps,
            })));
        }
    }
    Ok(Json(None))
}

async fn report_step(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
    Json(req): Json<StepReport>,
) -> ApiResult<StepReportResponse> {
    let id = parse_run_id(&run_id)?;
    let Some(run) = state
        .store
        .get_run(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    else {
        return Err(not_found(&run_id));
    };
    if run.cancel_requested {
        return Err((StatusCode::CONFLICT, "run cancel requested".to_string()));
    }
    let rec = StepRecord {
        run_id: id,
        key: req.key.clone(),
        result: req.result.clone(),
        idempotency_key: ultraflows_core::idempotency_key(id, &req.key),
    };
    let recorded = state
        .store
        .record_step(&rec)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if recorded {
        state
            .store
            .append_event(id, "StepRecorded", serde_json::json!({"key": req.key}))
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(StepReportResponse {
            recorded: true,
            result: req.result,
        }))
    } else {
        let existing = state
            .store
            .get_step(id, &req.key)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .map(|r| r.result)
            .unwrap_or(req.result);
        Ok(Json(StepReportResponse {
            recorded: false,
            result: existing,
        }))
    }
}

async fn report_wait(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
    Json(req): Json<WaitReport>,
) -> ApiResult<serde_json::Value> {
    let id = parse_run_id(&run_id)?;
    let Some(mut run) = state
        .store
        .get_run(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    else {
        return Err(not_found(&run_id));
    };
    if run.status != RunStatus::Running {
        return Err((
            StatusCode::CONFLICT,
            format!("run is {:?}, not Running", run.status),
        ));
    }
    run.status = RunStatus::Waiting;
    run.wait_key = Some(req.key.clone());
    run.wait_kind = Some(req.kind.clone());
    run.wait_token = req.token.clone();
    run.wake_at = req.wake_at;
    run.leased_by = None;
    run.lease_expires_at = None;
    run.updated_at = Utc::now();
    state
        .store
        .update_run(&run)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state
        .store
        .upsert_wait(&WaitRecord {
            run_id: id,
            key: req.key.clone(),
            kind: req.kind.clone(),
            token: req.token.clone(),
            wake_at: req.wake_at,
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state
        .store
        .append_event(
            id,
            "Suspended",
            serde_json::json!({"key": req.key, "kind": req.kind}),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn complete_run(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
    Json(req): Json<CompleteReport>,
) -> ApiResult<serde_json::Value> {
    let id = parse_run_id(&run_id)?;
    let Some(mut run) = state
        .store
        .get_run(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    else {
        return Err(not_found(&run_id));
    };
    if run.cancel_requested {
        run.status = RunStatus::Cancelled;
        run.output = None;
    } else {
        run.status = RunStatus::Completed;
        run.output = Some(req.output);
    }
    run.leased_by = None;
    run.lease_expires_at = None;
    run.updated_at = Utc::now();
    state
        .store
        .update_run(&run)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state
        .store
        .append_event(id, "Completed", serde_json::json!({}))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    // triggerAndWait parents wake on child completion.
    if run.status == RunStatus::Completed {
        let parents = state
            .store
            .wake_child_parents(id, Utc::now().timestamp_millis())
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        for p in parents {
            let _ = state
                .store
                .append_event(p, "Woken", serde_json::json!({"child": id}))
                .await;
        }
    }
    Ok(Json(serde_json::json!({"ok": true, "status": run.status})))
}

async fn fail_run(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
    Json(req): Json<FailReport>,
) -> ApiResult<serde_json::Value> {
    let id = parse_run_id(&run_id)?;
    let Some(mut run) = state
        .store
        .get_run(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    else {
        return Err(not_found(&run_id));
    };
    if let (Some(want), Some(have)) = (&req.worker_id, &run.leased_by) {
        if want != have {
            return Err((
                StatusCode::CONFLICT,
                "lease held by another worker".to_string(),
            ));
        }
    }
    run.leased_by = None;
    run.lease_expires_at = None;
    run.error = Some(req.error.clone());
    run.updated_at = Utc::now();
    if run.cancel_requested {
        run.status = RunStatus::Cancelled;
        run.cancel_requested = false;
        state
            .store
            .update_run(&run)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        state
            .store
            .append_event(id, "Cancelled", serde_json::json!({}))
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        return Ok(Json(serde_json::json!({"ok": true, "status": run.status})));
    }
    let policy = state
        .store
        .get_task(&run.workflow)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(|t| t.retry)
        .unwrap_or_default();
    if policy.exhausted(run.attempt) {
        run.status = RunStatus::Failed;
        state
            .store
            .update_run(&run)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        state
            .store
            .append_event(id, "Failed", serde_json::json!({"error": req.error}))
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    } else {
        let delay = if policy.randomize {
            let seed =
                (id.as_u128() as u64) ^ (run.attempt as u64).wrapping_mul(0x9E3779B97F4A7C15);
            policy.delay_with_jitter(run.attempt, seed)
        } else {
            policy.delay_after(run.attempt)
        };
        run.status = RunStatus::Pending;
        run.next_attempt_at =
            Some(Utc::now() + chrono::Duration::from_std(delay).unwrap_or_default());
        state
            .store
            .update_run(&run)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        state
            .store
            .append_event(
                id,
                "RetryScheduled",
                serde_json::json!({"attempt": run.attempt, "delay_ms": delay.as_millis()}),
            )
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    Ok(Json(serde_json::json!({"ok": true, "status": run.status})))
}

async fn lease_heartbeat(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
    Json(req): Json<HeartbeatReport>,
) -> ApiResult<serde_json::Value> {
    let id = parse_run_id(&run_id)?;
    let Some(mut run) = state
        .store
        .get_run(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    else {
        return Err(not_found(&run_id));
    };
    if run.status != RunStatus::Running {
        return Err((
            StatusCode::CONFLICT,
            format!("run is {:?}, not Running", run.status),
        ));
    }
    if let (Some(want), Some(have)) = (&req.worker_id, &run.leased_by) {
        if want != have {
            return Err((
                StatusCode::CONFLICT,
                "lease held by another worker".to_string(),
            ));
        }
    }
    run.lease_expires_at = Some(Utc::now() + chrono::Duration::milliseconds(LEASE_MS));
    run.updated_at = Utc::now();
    state
        .store
        .update_run(&run)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

// ---------- trigger API ----------

async fn trigger(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
    Json(opts): Json<TriggerOptions>,
) -> ApiResult<serde_json::Value> {
    let (run, created) = trigger_task(&state.store, &task_id, opts, None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({
        "run_id": run.id, "status": run.status, "created": created,
    })))
}

async fn batch_trigger(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
    Json(req): Json<BatchRequest>,
) -> ApiResult<Vec<BatchItemResult>> {
    if req.items.len() > BATCH_MAX {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("batch exceeds cap of {BATCH_MAX} items"),
        ));
    }
    let mut out = Vec::new();
    for item in req.items {
        let (run, created) = trigger_task(&state.store, &task_id, item.opts, None)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        out.push(BatchItemResult {
            run_id: run.id,
            status: run.status,
            created,
        });
    }
    Ok(Json(out))
}

async fn list_runs(
    State(state): State<Arc<AppState>>,
    Query(f): Query<RunFilters>,
) -> ApiResult<Vec<Run>> {
    let runs = state
        .store
        .list_runs_filtered(f.status.as_deref(), f.task.as_deref(), f.from, f.to)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let runs = match &f.tag {
        Some(tag) => runs
            .into_iter()
            .filter(|r| r.tags.iter().any(|t| t == tag))
            .collect(),
        None => runs,
    };
    Ok(Json(runs))
}

async fn get_run(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult<Run> {
    let id = parse_run_id(&id)?;
    state
        .store
        .get_run(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .ok_or_else(|| not_found(&id.to_string()))
}

async fn get_events(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Vec<ultraflows_core::types::RunEvent>> {
    let id = parse_run_id(&id)?;
    Ok(Json(state.store.list_events(id).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?))
}

async fn get_steps(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Vec<StepRecord>> {
    let id = parse_run_id(&id)?;
    Ok(Json(state.store.list_steps(id).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?))
}

async fn get_wait(
    State(state): State<Arc<AppState>>,
    Path((id, key)): Path<(String, String)>,
) -> ApiResult<WaitRecord> {
    let id = parse_run_id(&id)?;
    state
        .store
        .get_wait(id, &key)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .ok_or_else(|| not_found(&key))
}

async fn cancel_run(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<serde_json::Value> {
    let id = parse_run_id(&id)?;
    let existed = state
        .store
        .request_cancel(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !existed {
        return Err(not_found(&id.to_string()));
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn replay_run(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<ReplayRequest>,
) -> ApiResult<serde_json::Value> {
    let id = parse_run_id(&id)?;
    let Some(src) = state
        .store
        .get_run(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    else {
        return Err(not_found(&id.to_string()));
    };
    if !src.status.is_terminal() {
        return Err((
            StatusCode::CONFLICT,
            format!("source run is {:?}, not terminal", src.status),
        ));
    }
    let (run, _) = trigger_task(
        &state.store,
        &src.workflow,
        TriggerOptions {
            payload: Some(req.payload.unwrap_or(src.payload.clone())),
            max_attempts: Some(src.max_attempts),
            concurrency_key: src.concurrency_key.clone(),
            tags: Some(src.tags.clone()),
            metadata: Some(src.metadata.clone()),
            ..Default::default()
        },
        None,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    // Link the replay to its source (trigger_task does not carry it).
    let mut r = run.clone();
    r.source_run_id = Some(src.id);
    r.updated_at = Utc::now();
    let _ = state.store.update_run(&r).await;
    Ok(Json(serde_json::json!({
        "run_id": run.id, "status": run.status, "source_run_id": src.id,
    })))
}

async fn reschedule_run(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<RescheduleRequest>,
) -> ApiResult<serde_json::Value> {
    let id = parse_run_id(&id)?;
    let Some(mut run) = state
        .store
        .get_run(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    else {
        return Err(not_found(&id.to_string()));
    };
    // Delayed, not-yet-started only: Pending with zero attempts so far.
    if run.status != RunStatus::Pending || run.attempt != 0 {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "only a delayed, not-yet-started run can be rescheduled (status {:?}, attempt {})",
                run.status, run.attempt
            ),
        ));
    }
    run.next_attempt_at = Some(Utc::now() + chrono::Duration::milliseconds(req.delay_ms as i64));
    run.updated_at = Utc::now();
    state
        .store
        .update_run(&run)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

// ---------- schedules ----------

fn compute_next_fire(cron: &str, tz: &str, from: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
    next_fire(cron, tz, from)
}

async fn create_schedule(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ScheduleInput>,
) -> ApiResult<Schedule> {
    let now = Utc::now();
    let next = compute_next_fire(&req.cron, &req.timezone, now)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let sched = Schedule {
        id: Uuid::now_v7().to_string(),
        task_id: req.task_id,
        cron: req.cron,
        timezone: req.timezone,
        external_id: req.external_id,
        deduplication_key: req.deduplication_key.clone(),
        payload: req.payload.unwrap_or(serde_json::json!({})),
        enabled: true,
        next_fire_at: Some(next),
        created_at: now,
        updated_at: now,
    };
    let saved = if req.deduplication_key.is_some() {
        state
            .store
            .upsert_schedule_by_dedup(&sched)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    } else {
        state
            .store
            .create_schedule(&sched)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        sched
    };
    Ok(Json(saved))
}

async fn patch_schedule(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<SchedulePatch>,
) -> ApiResult<Schedule> {
    let Some(mut sched) = state
        .store
        .get_schedule(&id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    else {
        return Err(not_found(&id));
    };
    if let Some(cron) = req.cron {
        sched.cron = cron;
    }
    if let Some(tz) = req.timezone {
        sched.timezone = tz;
    }
    if let Some(p) = req.payload {
        sched.payload = p;
    }
    if let Some(en) = req.enabled {
        sched.enabled = en;
    }
    sched.next_fire_at = Some(
        compute_next_fire(&sched.cron, &sched.timezone, Utc::now())
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?,
    );
    sched.updated_at = Utc::now();
    state
        .store
        .update_schedule(&sched)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(sched))
}

async fn delete_schedule(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<serde_json::Value> {
    let deleted = state
        .store
        .delete_schedule(&id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !deleted {
        return Err(not_found(&id));
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn list_schedules(State(state): State<Arc<AppState>>) -> ApiResult<Vec<Schedule>> {
    Ok(Json(state.store.list_schedules().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?))
}

// ---------- waitpoints ----------

async fn complete_waitpoint(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
    Json(req): Json<WaitpointComplete>,
) -> ApiResult<serde_json::Value> {
    let woken = state
        .store
        .record_approval(
            &token,
            req.output.unwrap_or(serde_json::json!({})),
            Utc::now().timestamp_millis(),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({"ok": true, "woken_runs": woken})))
}

async fn get_waitpoint(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> ApiResult<serde_json::Value> {
    let result = state
        .store
        .get_approval(&token)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    match result {
        Some(v) => Ok(Json(serde_json::json!({"completed": true, "output": v}))),
        None => Ok(Json(serde_json::json!({"completed": false}))),
    }
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/workers/register", post(register_worker))
        .route("/v1/workers/{worker_id}/heartbeat", post(worker_heartbeat))
        .route("/v1/workers/{worker_id}/lease", post(lease_work))
        .route("/v1/workers/leases/{run_id}/step", post(report_step))
        .route("/v1/workers/leases/{run_id}/wait", post(report_wait))
        .route("/v1/workers/leases/{run_id}/complete", post(complete_run))
        .route("/v1/workers/leases/{run_id}/fail", post(fail_run))
        .route(
            "/v1/workers/leases/{run_id}/heartbeat",
            post(lease_heartbeat),
        )
        .route("/v1/tasks/{id}/trigger", post(trigger))
        .route("/v1/tasks/{id}/batch-trigger", post(batch_trigger))
        .route("/v1/runs", get(list_runs))
        .route("/v1/runs/{id}", get(get_run))
        .route("/v1/runs/{id}/events", get(get_events))
        .route("/v1/runs/{id}/steps", get(get_steps))
        .route("/v1/runs/{id}/waits/{key}", get(get_wait))
        .route("/v1/runs/{id}/cancel", post(cancel_run))
        .route("/v1/runs/{id}/replay", post(replay_run))
        .route("/v1/runs/{id}/reschedule", post(reschedule_run))
        .route("/v1/schedules", post(create_schedule).get(list_schedules))
        .route(
            "/v1/schedules/{id}",
            patch(patch_schedule).delete(delete_schedule),
        )
        .route("/v1/waitpoints/{token}/complete", post(complete_waitpoint))
        .route("/v1/waitpoints/{token}", get(get_waitpoint))
        .route("/v1/approvals/{token}", post(complete_waitpoint))
        .with_state(Arc::new(state))
}

/// Scheduler tick (called by main's loop): expire TTLs, then fire due
/// schedules. A schedule fire creates one run per fire time and
/// advances `next_fire_at` from the fired time (not from now), so a
/// slow tick does not silently skip or duplicate fires.
pub async fn scheduler_tick(store: &SqliteStore) -> Result<Vec<Uuid>, String> {
    let now = Utc::now();
    let _ = store
        .expire_ttl(now.timestamp_millis())
        .await
        .map_err(|e| e.to_string())?;
    let mut fired = Vec::new();
    let schedules = store.list_schedules().await.map_err(|e| e.to_string())?;
    for mut sched in schedules {
        if !sched.enabled {
            continue;
        }
        let Some(fire_at) = sched.next_fire_at else {
            continue;
        };
        if fire_at > now {
            continue;
        }
        let (run, _) = trigger_task(
            store,
            &sched.task_id,
            TriggerOptions {
                payload: Some(sched.payload.clone()),
                ..Default::default()
            },
            Some(sched.id.clone()),
        )
        .await?;
        fired.push(run.id);
        sched.next_fire_at = next_fire(&sched.cron, &sched.timezone, fire_at).ok();
        sched.updated_at = Utc::now();
        store
            .update_schedule(&sched)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(fired)
}
