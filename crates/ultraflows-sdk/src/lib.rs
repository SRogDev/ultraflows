//! UltraFlows Rust SDK — the mechanism for other apps to send jobs.
//!
//! Two roles, one client:
//! - **Triggering**: `Client::trigger` / `batch_trigger` / schedules /
//!   waitpoint completion — what an app calls to start work.
//! - **Working**: `Worker` leases runs, executes task functions with a
//!   `RemoteCtx`, and reports step results, waits, completion and
//!   failure back to the server over HTTP.
//!
//! `RemoteCtx` mirrors the embedded `Ctx` model: `step` is journaled
//! (insert-once on the server, so a re-executed task never re-runs a
//! recorded step), `sleep` / `wait_until` / `wait_for_token` suspend
//! the run until the timer, token, or child run resolves, and
//! `trigger_and_wait` runs a child task and suspends on its completion.
//!
//! This SDK speaks UltraFlows' own protocol (see the server's `api`
//! module). It is NOT a Trigger.dev hosted-API client.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use ultraflows_core::RetryPolicy;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    #[error("suspended on wait {0}")]
    Suspended(String),
    #[error("task failed: {0}")]
    Failed(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("server error {0}: {1}")]
    Server(u16, String),
    #[error("serialization error: {0}")]
    Serde(String),
}

type Result<T> = std::result::Result<T, SdkError>;

fn http_err(e: reqwest::Error) -> SdkError {
    SdkError::Http(e.to_string())
}

fn serde_err(e: serde_json::Error) -> SdkError {
    SdkError::Serde(e.to_string())
}

// ---------- wire types (mirror the server API; the SDK does not depend
// on the server crate, so these are intentionally duplicated) ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDef {
    pub id: String,
    pub version_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<RetryPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency_total: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency_per_key: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TriggerOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_ttl_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_attempts: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct TriggerResponse {
    pub run_id: Uuid,
    pub status: String,
    pub created: bool,
}

#[derive(Debug, Deserialize)]
pub struct RegisterResponse {
    pub worker_id: String,
    pub tasks: Vec<String>,
    pub promoted_runs: Vec<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RunView {
    pub id: Uuid,
    pub workflow: String,
    pub payload: serde_json::Value,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub status: String,
    pub attempt: u32,
    pub max_attempts: u32,
    pub wait_key: Option<String>,
    pub wait_kind: Option<String>,
    pub wait_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StepView {
    pub run_id: Uuid,
    pub key: String,
    pub result: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct LeasedRun {
    pub run: RunView,
    pub steps: Vec<StepView>,
}

#[derive(Debug, Deserialize)]
pub struct StepReportResponse {
    pub recorded: bool,
    pub result: serde_json::Value,
}

// ---------- client ----------

#[derive(Clone)]
pub struct Client {
    base: String,
    http: reqwest::Client,
}

impl Client {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    async fn check(resp: reqwest::Response) -> Result<reqwest::Response> {
        let status = resp.status();
        if status.is_success() {
            Ok(resp)
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(SdkError::Server(status.as_u16(), body))
        }
    }

    pub async fn register(&self, worker_id: &str, tasks: Vec<TaskDef>) -> Result<RegisterResponse> {
        let resp = self
            .http
            .post(self.url("/v1/workers/register"))
            .json(&serde_json::json!({"worker_id": worker_id, "tasks": tasks}))
            .send()
            .await
            .map_err(http_err)?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(http_err)
    }

    pub async fn worker_heartbeat(&self, worker_id: &str) -> Result<()> {
        let resp = self
            .http
            .post(self.url(&format!("/v1/workers/{worker_id}/heartbeat")))
            .send()
            .await
            .map_err(http_err)?;
        Self::check(resp).await?;
        Ok(())
    }

    pub async fn trigger(&self, task_id: &str, opts: TriggerOptions) -> Result<TriggerResponse> {
        let resp = self
            .http
            .post(self.url(&format!("/v1/tasks/{task_id}/trigger")))
            .json(&opts)
            .send()
            .await
            .map_err(http_err)?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(http_err)
    }

    pub async fn batch_trigger(
        &self,
        task_id: &str,
        items: Vec<TriggerOptions>,
    ) -> Result<Vec<serde_json::Value>> {
        let resp = self
            .http
            .post(self.url(&format!("/v1/tasks/{task_id}/batch-trigger")))
            .json(&serde_json::json!({"items": items}))
            .send()
            .await
            .map_err(http_err)?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(http_err)
    }

    pub async fn get_run(&self, run_id: Uuid) -> Result<RunView> {
        let resp = self
            .http
            .get(self.url(&format!("/v1/runs/{run_id}")))
            .send()
            .await
            .map_err(http_err)?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(http_err)
    }

    pub async fn list_runs(&self, query: &str) -> Result<Vec<RunView>> {
        let resp = self
            .http
            .get(self.url(&format!("/v1/runs{query}")))
            .send()
            .await
            .map_err(http_err)?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(http_err)
    }

    pub async fn cancel_run(&self, run_id: Uuid) -> Result<()> {
        let resp = self
            .http
            .post(self.url(&format!("/v1/runs/{run_id}/cancel")))
            .send()
            .await
            .map_err(http_err)?;
        Self::check(resp).await?;
        Ok(())
    }

    pub async fn replay_run(
        &self,
        run_id: Uuid,
        payload: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(self.url(&format!("/v1/runs/{run_id}/replay")))
            .json(&serde_json::json!({"payload": payload}))
            .send()
            .await
            .map_err(http_err)?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(http_err)
    }

    pub async fn complete_waitpoint(
        &self,
        token: &str,
        output: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(self.url(&format!("/v1/waitpoints/{token}/complete")))
            .json(&serde_json::json!({"output": output}))
            .send()
            .await
            .map_err(http_err)?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(http_err)
    }

    pub async fn get_waitpoint(&self, token: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .get(self.url(&format!("/v1/waitpoints/{token}")))
            .send()
            .await
            .map_err(http_err)?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(http_err)
    }

    pub async fn create_schedule(
        &self,
        task_id: &str,
        cron: &str,
        timezone: &str,
        payload: serde_json::Value,
        deduplication_key: Option<&str>,
    ) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(self.url("/v1/schedules"))
            .json(&serde_json::json!({
                "task_id": task_id, "cron": cron, "timezone": timezone,
                "payload": payload, "deduplication_key": deduplication_key,
            }))
            .send()
            .await
            .map_err(http_err)?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(http_err)
    }

    pub async fn lease(&self, worker_id: &str, tasks: &[String]) -> Result<Option<LeasedRun>> {
        let resp = self
            .http
            .post(self.url(&format!("/v1/workers/{worker_id}/lease")))
            .json(&serde_json::json!({"tasks": tasks}))
            .send()
            .await
            .map_err(http_err)?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(http_err)
    }

    pub async fn report_step(
        &self,
        run_id: Uuid,
        key: &str,
        result: serde_json::Value,
    ) -> Result<StepReportResponse> {
        let resp = self
            .http
            .post(self.url(&format!("/v1/workers/leases/{run_id}/step")))
            .json(&serde_json::json!({"key": key, "result": result}))
            .send()
            .await
            .map_err(http_err)?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(http_err)
    }

    pub async fn report_wait(
        &self,
        run_id: Uuid,
        key: &str,
        kind: &str,
        wake_at: Option<DateTime<Utc>>,
        token: Option<&str>,
    ) -> Result<()> {
        let resp = self
            .http
            .post(self.url(&format!("/v1/workers/leases/{run_id}/wait")))
            .json(&serde_json::json!({
                "key": key, "kind": kind, "wake_at": wake_at, "token": token,
            }))
            .send()
            .await
            .map_err(http_err)?;
        Self::check(resp).await?;
        Ok(())
    }

    pub async fn report_complete(&self, run_id: Uuid, output: serde_json::Value) -> Result<()> {
        let resp = self
            .http
            .post(self.url(&format!("/v1/workers/leases/{run_id}/complete")))
            .json(&serde_json::json!({"output": output}))
            .send()
            .await
            .map_err(http_err)?;
        Self::check(resp).await?;
        Ok(())
    }

    pub async fn report_fail(&self, run_id: Uuid, worker_id: &str, error: &str) -> Result<()> {
        let resp = self
            .http
            .post(self.url(&format!("/v1/workers/leases/{run_id}/fail")))
            .json(&serde_json::json!({"error": error, "worker_id": worker_id}))
            .send()
            .await
            .map_err(http_err)?;
        Self::check(resp).await?;
        Ok(())
    }

    pub async fn lease_heartbeat(&self, run_id: Uuid, worker_id: &str) -> Result<()> {
        let resp = self
            .http
            .post(self.url(&format!("/v1/workers/leases/{run_id}/heartbeat")))
            .json(&serde_json::json!({"worker_id": worker_id}))
            .send()
            .await
            .map_err(http_err)?;
        Self::check(resp).await?;
        Ok(())
    }

    pub async fn get_wait(&self, run_id: Uuid, key: &str) -> Result<Option<serde_json::Value>> {
        let resp = self
            .http
            .get(self.url(&format!("/v1/runs/{run_id}/waits/{key}")))
            .send()
            .await
            .map_err(http_err)?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(http_err)
    }
}

// ---------- remote context ----------

/// Execution context for a leased run. Step results are journaled on
/// the server: on re-execution after a wait or retry, `step` returns
/// the recorded result without re-running the closure.
#[derive(Clone)]
pub struct RemoteCtx {
    client: Client,
    run_id: Uuid,
    steps: Arc<HashMap<String, serde_json::Value>>,
    attempt: u32,
}

impl RemoteCtx {
    pub fn new(client: Client, run_id: Uuid, steps: Vec<StepView>, attempt: u32) -> Self {
        let map = steps.into_iter().map(|s| (s.key, s.result)).collect();
        Self {
            client,
            run_id,
            steps: Arc::new(map),
            attempt,
        }
    }

    pub fn run_id(&self) -> Uuid {
        self.run_id
    }

    /// 1-based attempt number of the current lease.
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub async fn step<T, F, Fut>(&self, key: &str, f: F) -> std::result::Result<T, SdkError>
    where
        T: serde::de::DeserializeOwned + serde::Serialize,
        F: FnOnce() -> Fut,
        Fut: Future<Output = std::result::Result<T, String>>,
    {
        if let Some(recorded) = self.steps.get(key) {
            return serde_json::from_value(recorded.clone()).map_err(serde_err);
        }
        let value = f().await.map_err(SdkError::Failed)?;
        let json = serde_json::to_value(&value).map_err(serde_err)?;
        let reported = self.client.report_step(self.run_id, key, json).await?;
        // Insert-once: a racing duplicate report returns the recorded
        // result, which wins over this attempt's value.
        serde_json::from_value(reported.result).map_err(serde_err)
    }

    async fn suspend(
        &self,
        key: &str,
        kind: &str,
        wake_at: Option<DateTime<Utc>>,
        token: Option<&str>,
    ) -> SdkError {
        match self
            .client
            .report_wait(self.run_id, key, kind, wake_at, token)
            .await
        {
            Ok(()) => SdkError::Suspended(key.to_string()),
            Err(e) => e,
        }
    }

    pub async fn sleep(&self, key: &str, duration: Duration) -> std::result::Result<(), SdkError> {
        let wake_at = Utc::now() + chrono::Duration::from_std(duration).unwrap_or_default();
        Err(self.suspend(key, "timer", Some(wake_at), None).await)
    }

    pub async fn wait_until(
        &self,
        key: &str,
        when: DateTime<Utc>,
    ) -> std::result::Result<(), SdkError> {
        Err(self.suspend(key, "timer", Some(when), None).await)
    }

    /// Wait for an external token (approval). If the token was already
    /// completed, returns its output immediately instead of suspending.
    pub async fn wait_for_token(
        &self,
        key: &str,
        token: &str,
    ) -> std::result::Result<serde_json::Value, SdkError> {
        let wp = self.client.get_waitpoint(token).await?;
        if wp
            .get("completed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return Ok(wp.get("output").cloned().unwrap_or(serde_json::json!({})));
        }
        Err(self.suspend(key, "approval", None, Some(token)).await)
    }

    /// Trigger a child run and suspend until it completes. On
    /// re-execution, the recorded child id is reused (no duplicate
    /// child) and its output returned once completed.
    pub async fn trigger_and_wait(
        &self,
        key: &str,
        task_id: &str,
        payload: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, SdkError> {
        if let Some(wait) = self.client.get_wait(self.run_id, key).await? {
            if let Some(child_id) = wait.get("token").and_then(|v| v.as_str()) {
                let child_id =
                    Uuid::parse_str(child_id).map_err(|e| SdkError::Serde(e.to_string()))?;
                let child = self.client.get_run(child_id).await?;
                match child.status.as_str() {
                    "Completed" => {
                        return Ok(child.output.unwrap_or(serde_json::json!({})));
                    }
                    "Failed" | "Cancelled" | "Expired" => {
                        return Err(SdkError::Failed(format!(
                            "child run {} ended as {}",
                            child_id, child.status
                        )));
                    }
                    _ => {
                        // Still running/waiting: re-suspend on it.
                        return Err(self
                            .suspend(key, "child", None, Some(child_id.to_string().as_str()))
                            .await);
                    }
                }
            }
        }
        let child = self
            .client
            .trigger(
                task_id,
                TriggerOptions {
                    payload: Some(payload),
                    ..Default::default()
                },
            )
            .await?;
        Err(self
            .suspend(key, "child", None, Some(child.run_id.to_string().as_str()))
            .await)
    }
}

// ---------- worker ----------

pub type TaskFn = Arc<
    dyn Fn(
            RemoteCtx,
            serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value>> + Send>>
        + Send
        + Sync,
>;

pub struct Worker {
    client: Client,
    worker_id: String,
    defs: Vec<TaskDef>,
    tasks: HashMap<String, TaskFn>,
}

impl Worker {
    pub fn new(client: Client, worker_id: impl Into<String>) -> Self {
        Self {
            client,
            worker_id: worker_id.into(),
            defs: Vec::new(),
            tasks: HashMap::new(),
        }
    }

    pub fn task(mut self, def: TaskDef, f: TaskFn) -> Self {
        self.tasks.insert(def.id.clone(), f);
        self.defs.push(def);
        self
    }

    pub fn task_ids(&self) -> Vec<String> {
        self.tasks.keys().cloned().collect()
    }

    pub async fn register(&self) -> Result<RegisterResponse> {
        self.client
            .register(&self.worker_id, self.defs.clone())
            .await
    }

    /// Lease one run and execute it to completion, failure (server
    /// retries per the task policy), or suspension. Returns the run
    /// id, or None when no work was available.
    pub async fn run_once(&self) -> Result<Option<Uuid>> {
        let Some(leased) = self.client.lease(&self.worker_id, &self.task_ids()).await? else {
            return Ok(None);
        };
        let run_id = leased.run.id;
        let Some(f) = self.tasks.get(&leased.run.workflow) else {
            // Should not happen: lease only returns declared tasks.
            // Fail it so it retries instead of wedging the queue.
            self.client
                .report_fail(run_id, &self.worker_id, "worker has no such task")
                .await?;
            return Ok(Some(run_id));
        };
        let ctx = RemoteCtx::new(
            self.client.clone(),
            run_id,
            leased.steps,
            leased.run.attempt,
        );
        let payload = leased.run.payload.clone();
        // Heartbeat the lease (30 s) every 10 s while the task runs.
        let hb_client = self.client.clone();
        let hb_worker = self.worker_id.clone();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done_flag = done.clone();
        let hb = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                if done_flag.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                let _ = hb_client.lease_heartbeat(run_id, &hb_worker).await;
            }
        });
        let result = f(ctx, payload).await;
        done.store(true, std::sync::atomic::Ordering::SeqCst);
        hb.abort();
        match result {
            Ok(output) => {
                self.client.report_complete(run_id, output).await?;
            }
            Err(SdkError::Suspended(_)) => {
                // The wait was already reported by RemoteCtx; nothing
                // more to do — the run is Waiting on the server.
            }
            Err(SdkError::Failed(msg)) => {
                self.client
                    .report_fail(run_id, &self.worker_id, &msg)
                    .await?;
            }
            Err(e) => {
                // Transport/serialization errors: fail the run so the
                // server can retry it instead of losing the lease.
                self.client
                    .report_fail(run_id, &self.worker_id, &format!("worker error: {e}"))
                    .await?;
            }
        }
        Ok(Some(run_id))
    }

    /// Drive forever, polling for leased work.
    pub async fn drive(&self, poll_interval: Duration) {
        loop {
            match self.run_once().await {
                Ok(Some(_)) => continue,
                Ok(None) => tokio::time::sleep(poll_interval).await,
                Err(e) => {
                    tracing::error!("worker error: {e}");
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }
    }
}
