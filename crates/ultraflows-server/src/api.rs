//! HTTP API (v0.1.0, minimal).
//!
//! - POST /v1/runs — trigger a workflow
//! - GET /v1/runs — list runs
//! - GET /v1/runs/{id} — run detail (includes wait token when waiting
//!   on an approval, so a caller knows what to complete)
//! - GET /v1/runs/{id}/events — append-only event log
//! - GET /v1/runs/{id}/steps — journaled steps (the replay proof)
//! - POST /v1/runs/{id}/cancel — cancel (immediate when waiting/pending,
//!   at the next step boundary when running)
//! - POST /v1/approvals/{token} — complete a human-approval waitpoint
//! - GET /health
//!
//! No authentication in v0.1.0: this is a local demo server, and the
//! README says so. Do not expose it to a network you do not trust.

use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, post};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use ultraflows::Executor;
use ultraflows_core::types::{NewRun, Run};
use ultraflows_store::{SqliteStore, Store};

#[derive(Clone)]
pub struct AppState {
    pub executor: Executor<SqliteStore>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRunRequest {
    pub workflow: String,
    pub payload: Option<serde_json::Value>,
    pub max_attempts: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct CreateRunResponse {
    pub run_id: uuid::Uuid,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct ApproveRequest {
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct ApproveResponse {
    pub woken_runs: Vec<uuid::Uuid>,
}

type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;

fn err(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

async fn health() -> Json<serde_json::Value> {
    Json(
        serde_json::json!({"status": "ok", "service": "ultraflows", "version": env!("CARGO_PKG_VERSION")}),
    )
}

async fn create_run(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateRunRequest>,
) -> ApiResult<CreateRunResponse> {
    if !crate::workflows::names().contains(&req.workflow.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "unknown workflow: {}. Known: {:?}",
                req.workflow,
                crate::workflows::names()
            ),
        ));
    }
    let store = state.executor.store();
    let run = store
        .create_run(NewRun {
            workflow: req.workflow,
            payload: req.payload.unwrap_or(serde_json::json!({})),
            max_attempts: req.max_attempts.unwrap_or(5),
        })
        .await
        .map_err(err)?;
    store
        .append_event(run.id, "Created", serde_json::json!({}))
        .await
        .map_err(err)?;
    Ok(Json(CreateRunResponse {
        run_id: run.id,
        status: format!("{:?}", run.status),
    }))
}

async fn list_runs(State(state): State<Arc<AppState>>) -> ApiResult<Vec<Run>> {
    let runs = state.executor.store().list_runs().await.map_err(err)?;
    Ok(Json(runs))
}

async fn get_run(State(state): State<Arc<AppState>>, Path(id): Path<uuid::Uuid>) -> ApiResult<Run> {
    match state.executor.store().get_run(id).await.map_err(err)? {
        Some(run) => Ok(Json(run)),
        None => Err((StatusCode::NOT_FOUND, format!("run not found: {id}"))),
    }
}

async fn list_events(
    State(state): State<Arc<AppState>>,
    Path(id): Path<uuid::Uuid>,
) -> ApiResult<Vec<ultraflows_core::types::RunEvent>> {
    let events = state.executor.store().list_events(id).await.map_err(err)?;
    Ok(Json(events))
}

async fn list_steps(
    State(state): State<Arc<AppState>>,
    Path(id): Path<uuid::Uuid>,
) -> ApiResult<Vec<ultraflows_core::types::StepRecord>> {
    let steps = state.executor.store().list_steps(id).await.map_err(err)?;
    Ok(Json(steps))
}

async fn cancel_run(
    State(state): State<Arc<AppState>>,
    Path(id): Path<uuid::Uuid>,
) -> ApiResult<serde_json::Value> {
    let existed = state
        .executor
        .store()
        .request_cancel(id)
        .await
        .map_err(err)?;
    if !existed {
        return Err((StatusCode::NOT_FOUND, format!("run not found: {id}")));
    }
    let run = state.executor.store().get_run(id).await.map_err(err)?;
    Ok(Json(
        serde_json::json!({"run_id": id, "status": run.map(|r| format!("{:?}", r.status))}),
    ))
}

async fn approve(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
    Json(req): Json<ApproveRequest>,
) -> ApiResult<ApproveResponse> {
    let result = req.result.unwrap_or(serde_json::json!({"approved": true}));
    let woken = state
        .executor
        .store()
        .record_approval(&token, result, Utc::now().timestamp_millis())
        .await
        .map_err(err)?;
    Ok(Json(ApproveResponse { woken_runs: woken }))
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/runs", post(create_run).get(list_runs))
        .route("/v1/runs/{id}", get(get_run))
        .route("/v1/runs/{id}/events", get(list_events))
        .route("/v1/runs/{id}/steps", get(list_steps))
        .route("/v1/runs/{id}/cancel", post(cancel_run))
        .route("/v1/approvals/{token}", post(approve))
        .with_state(state)
}

// Note: (StatusCode, String) already implements IntoResponse in axum
// (String is a valid body), so no manual impl is needed — and adding
// one would conflict with axum's blanket implementation.
