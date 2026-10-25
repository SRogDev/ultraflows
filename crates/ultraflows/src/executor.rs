use crate::ctx::Ctx;
use crate::error::FlowError;
use chrono::Utc;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use ultraflows_core::types::{Run, RunStatus};
use ultraflows_core::{RetryPolicy, check_transition};
use ultraflows_store::{Store, StoreError};

/// A workflow function: deterministic orchestration code that receives a
/// context and its payload, and returns its output (or a `FlowError`).
pub type WorkflowFn<S> = Arc<
    dyn Fn(
            Ctx<S>,
            serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, FlowError>> + Send>>
        + Send
        + Sync,
>;

/// Construct a [`WorkflowFn`] from a closure. (A plain `Arc::new` does
/// not coerce to `Arc<dyn Fn>` in this position, so this helper exists
/// to keep workflow registration readable.)
pub fn workflow_fn<S: Store>(
    f: impl Fn(
        Ctx<S>,
        serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, FlowError>> + Send>>
    + Send
    + Sync
    + 'static,
) -> WorkflowFn<S> {
    Arc::new(f)
}

/// Drives runs: claims due runs, executes their workflow function,
/// finalizes them (Completed / Waiting / scheduled retry / Failed /
/// Cancelled) and appends the corresponding events.
///
/// v0.1.0 is a single worker: `run_once` claims at most one run, and
/// `drive` loops it. There is no distributed claiming, no lease
/// expiry — a crashed worker's Running run is reclaimed only by
/// restarting the worker (see the demo's kill -9 step, which restarts
/// it; a Running run left by a crash is *not* silently reclaimed in
/// this release — documented limitation).
#[derive(Clone)]
pub struct Executor<S: Store> {
    store: S,
    workflows: Arc<HashMap<String, WorkflowFn<S>>>,
    retry: RetryPolicy,
    /// Per-workflow retry overrides (from the task registry). Falls
    /// back to `retry` when a workflow has no entry. The remote-worker
    /// path retries server-side from the task definition instead; this
    /// map is for the embedded (library) executor only.
    policies: HashMap<String, RetryPolicy>,
}

impl<S: Store> Executor<S> {
    pub fn new(store: S, workflows: HashMap<String, WorkflowFn<S>>, retry: RetryPolicy) -> Self {
        Self {
            store,
            workflows: Arc::new(workflows),
            retry,
            policies: HashMap::new(),
        }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    /// Register a per-workflow retry policy override.
    pub fn with_policy(mut self, workflow: impl Into<String>, policy: RetryPolicy) -> Self {
        self.policies.insert(workflow.into(), policy);
        self
    }

    fn policy_for(&self, workflow: &str) -> &RetryPolicy {
        self.policies.get(workflow).unwrap_or(&self.retry)
    }

    /// Claim one due run and execute it to a terminal-for-now state.
    /// Returns the run id when a run was claimed, None when idle.
    pub async fn run_once(&self) -> Result<Option<uuid::Uuid>, StoreError> {
        let now_ms = Utc::now().timestamp_millis();
        // Promote PendingVersion runs whose task is now registered,
        // so a run triggered before its task existed becomes runnable
        // instead of sitting invisible forever.
        if let Ok(pending) = self
            .store
            .list_runs_filtered(Some("PendingVersion"), None, None, None)
            .await
        {
            for run in pending {
                if self
                    .store
                    .get_task(&run.workflow)
                    .await
                    .ok()
                    .flatten()
                    .is_some()
                {
                    let _ = self.store.promote_pending_version(&run.workflow).await;
                }
            }
        }
        let Some(run) = self.store.claim_due(now_ms).await? else {
            return Ok(None);
        };
        let run_id = run.id;
        if run.cancel_requested {
            self.finalize_cancelled(&run).await?;
            return Ok(Some(run_id));
        }
        self.store
            .append_event(
                run_id,
                "AttemptStarted",
                serde_json::json!({"attempt": run.attempt}),
            )
            .await?;
        let Some(workflow) = self.workflows.get(&run.workflow) else {
            self.finalize_failed(&run, &format!("unknown workflow: {}", run.workflow))
                .await?;
            return Ok(Some(run_id));
        };
        let ctx = Ctx::new(self.store.clone(), run_id, run.attempt);
        match workflow(ctx, run.payload.clone()).await {
            Ok(output) => self.finalize_completed(&run, output).await?,
            Err(FlowError::Suspended) => self.finalize_waiting(&run).await?,
            Err(FlowError::Cancelled) => self.finalize_cancelled(&run).await?,
            Err(FlowError::Failed(msg)) => {
                if self.policy_for(&run.workflow).exhausted(run.attempt) {
                    self.finalize_failed(&run, &msg).await?;
                } else {
                    self.finalize_retry(&run, &msg).await?;
                }
            }
            Err(FlowError::Store(e)) => return Err(e),
        }
        Ok(Some(run_id))
    }

    /// Drive forever, polling for due runs. Used by the server's worker
    /// task. `poll_interval` is the idle poll period (100 ms default).
    pub async fn drive(&self, poll_interval: Duration) {
        loop {
            match self.run_once().await {
                Ok(Some(_)) => continue, // work may be ready immediately
                Ok(None) => tokio::time::sleep(poll_interval).await,
                Err(e) => {
                    tracing::error!("executor error: {e}");
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }
    }

    /// Drive until `run_id` reaches a terminal state, or `deadline`
    /// passes. Test/helper method: returns the terminal run, or None
    /// on timeout.
    pub async fn run_until_terminal(&self, run_id: uuid::Uuid, deadline: Duration) -> Option<Run> {
        let start = tokio::time::Instant::now();
        loop {
            let _ = self.run_once().await;
            if let Ok(Some(run)) = self.store.get_run(run_id).await {
                if run.status.is_terminal() {
                    return Some(run);
                }
            }
            if start.elapsed() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    /// Drive until `run_id` reaches `status`, or `deadline` passes.
    pub async fn wait_for_status(
        &self,
        run_id: uuid::Uuid,
        status: RunStatus,
        deadline: Duration,
    ) -> Option<Run> {
        let start = tokio::time::Instant::now();
        loop {
            let _ = self.run_once().await;
            if let Ok(Some(run)) = self.store.get_run(run_id).await {
                if run.status == status {
                    return Some(run);
                }
            }
            if start.elapsed() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    async fn refresh(&self, run: &Run) -> Result<Run, StoreError> {
        self.store
            .get_run(run.id)
            .await?
            .ok_or(StoreError::NotFound(run.id))
    }

    async fn finalize_completed(
        &self,
        run: &Run,
        output: serde_json::Value,
    ) -> Result<(), StoreError> {
        let mut r = self.refresh(run).await?;
        if check_transition(r.status, RunStatus::Completed).is_ok() {
            r.status = RunStatus::Completed;
            r.output = Some(output);
            r.error = None;
            r.updated_at = Utc::now();
            self.store.update_run(&r).await?;
            self.store
                .append_event(r.id, "Completed", serde_json::json!({}))
                .await?;
        }
        Ok(())
    }

    async fn finalize_waiting(&self, run: &Run) -> Result<(), StoreError> {
        let mut r = self.refresh(run).await?;
        if check_transition(r.status, RunStatus::Waiting).is_ok() {
            r.status = RunStatus::Waiting;
            r.updated_at = Utc::now();
            self.store.update_run(&r).await?;
            self.store
                .append_event(r.id, "Suspended", serde_json::json!({}))
                .await?;
        }
        Ok(())
    }

    async fn finalize_cancelled(&self, run: &Run) -> Result<(), StoreError> {
        let mut r = self.refresh(run).await?;
        if check_transition(r.status, RunStatus::Cancelled).is_ok() {
            r.status = RunStatus::Cancelled;
            r.cancel_requested = false;
            r.updated_at = Utc::now();
            self.store.update_run(&r).await?;
            self.store
                .append_event(r.id, "Cancelled", serde_json::json!({}))
                .await?;
        }
        Ok(())
    }

    async fn finalize_failed(&self, run: &Run, msg: &str) -> Result<(), StoreError> {
        let mut r = self.refresh(run).await?;
        if check_transition(r.status, RunStatus::Failed).is_ok() {
            r.status = RunStatus::Failed;
            r.error = Some(msg.to_string());
            r.updated_at = Utc::now();
            self.store.update_run(&r).await?;
            self.store
                .append_event(r.id, "Failed", serde_json::json!({"error": msg}))
                .await?;
        }
        Ok(())
    }

    async fn finalize_retry(&self, run: &Run, msg: &str) -> Result<(), StoreError> {
        let mut r = self.refresh(run).await?;
        if check_transition(r.status, RunStatus::Pending).is_ok() {
            let delay = self.policy_for(&r.workflow).delay_after(r.attempt);
            r.status = RunStatus::Pending;
            r.error = Some(msg.to_string());
            r.next_attempt_at =
                Some(Utc::now() + chrono::Duration::from_std(delay).unwrap_or_default());
            r.updated_at = Utc::now();
            self.store.update_run(&r).await?;
            self.store
                .append_event(
                    r.id,
                    "RetryScheduled",
                    serde_json::json!({"attempt": r.attempt, "delay_ms": delay.as_millis()}),
                )
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use ultraflows_core::types::NewRun;
    use ultraflows_store::SqliteStore;

    fn flaky_workflow(counter: Arc<AtomicUsize>) -> WorkflowFn<SqliteStore> {
        Arc::new(move |ctx: Ctx<SqliteStore>, payload: serde_json::Value| {
            let counter = counter.clone();
            Box::pin(async move {
                let fail_times = payload["fail_times"].as_u64().unwrap_or(0) as u32;
                let attempt = ctx.attempt();
                let out: String = ctx
                    .step("generate", move || {
                        let counter = counter.clone();
                        async move {
                            counter.fetch_add(1, Ordering::SeqCst);
                            if attempt <= fail_times {
                                Err("simulated outage".to_string())
                            } else {
                                Ok("report-ready".to_string())
                            }
                        }
                    })
                    .await?;
                Ok(serde_json::json!({"report": out}))
            })
        })
    }

    #[tokio::test]
    async fn pending_version_run_promotes_and_executes_once_task_registered() {
        let store = SqliteStore::connect_memory().await.unwrap();
        store.migrate().await.unwrap();
        let counter = Arc::new(AtomicUsize::new(0));
        let mut workflows = HashMap::new();
        workflows.insert("flaky-report".to_string(), flaky_workflow(counter.clone()));
        let executor = Executor::new(store.clone(), workflows, RetryPolicy::default());
        let run = store
            .create_run(NewRun::new(
                "flaky-report",
                serde_json::json!({"fail_times": 0}),
                5,
            ))
            .await
            .unwrap();
        // Simulate a run triggered before its task was registered.
        let mut r = store.get_run(run.id).await.unwrap().unwrap();
        r.status = RunStatus::PendingVersion;
        store.update_run(&r).await.unwrap();
        // Unregistered: run_once claims nothing.
        assert!(executor.run_once().await.unwrap().is_none());
        // Register the task: the next run_once promotes and executes it.
        store
            .upsert_task(&ultraflows_core::types::TaskDefinition {
                id: "flaky-report".to_string(),
                version_hash: "v1".to_string(),
                retry: RetryPolicy::default(),
                concurrency_total: None,
                concurrency_per_key: None,
                queue: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .await
            .unwrap();
        let final_run = executor
            .run_until_terminal(run.id, Duration::from_secs(10))
            .await
            .expect("run should complete after registration");
        assert_eq!(final_run.status, RunStatus::Completed);
    }

    #[tokio::test]
    async fn flaky_workflow_retries_then_completes_without_reexecuting_recorded_steps() {
        let store = SqliteStore::connect_memory().await.unwrap();
        store.migrate().await.unwrap();
        let counter = Arc::new(AtomicUsize::new(0));
        let mut workflows = HashMap::new();
        workflows.insert("flaky-report".to_string(), flaky_workflow(counter.clone()));
        let executor = Executor::new(store.clone(), workflows, RetryPolicy::default());
        let run = store
            .create_run(NewRun::new(
                "flaky-report",
                serde_json::json!({"fail_times": 2}),
                5,
            ))
            .await
            .unwrap();
        store
            .append_event(run.id, "Created", serde_json::json!({}))
            .await
            .unwrap();
        let final_run = executor
            .run_until_terminal(run.id, Duration::from_secs(10))
            .await
            .expect("run should complete");
        assert_eq!(final_run.status, RunStatus::Completed);
        assert_eq!(final_run.attempt, 3);
        assert_eq!(final_run.output.unwrap()["report"], "report-ready");
        // The step closure ran once per attempt (failures record nothing),
        // and the successful result was recorded exactly once.
        assert_eq!(counter.load(Ordering::SeqCst), 3);
        assert_eq!(store.list_steps(run.id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn approval_wait_survives_executor_restart_and_resumes() {
        let store = SqliteStore::connect_memory().await.unwrap();
        store.migrate().await.unwrap();
        let workflow: WorkflowFn<SqliteStore> = Arc::new(|ctx, _| {
            Box::pin(async move {
                let _v: String = ctx
                    .step("prepare", || async { Ok("prepared".to_string()) })
                    .await?;
                let approval = ctx.wait_for_approval("approval", "token-1").await?;
                let done: String = ctx
                    .step("finish", || async { Ok("finished".to_string()) })
                    .await?;
                Ok(serde_json::json!({"approval": approval, "done": done}))
            })
        });
        let mut workflows = HashMap::new();
        workflows.insert("approval-demo".to_string(), workflow);
        let executor = Executor::new(store.clone(), workflows, RetryPolicy::default());
        let run = store
            .create_run(NewRun::new("approval-demo", serde_json::json!({}), 3))
            .await
            .unwrap();
        // First executor instance drives it to Waiting.
        executor
            .wait_for_status(run.id, RunStatus::Waiting, Duration::from_secs(2))
            .await
            .expect("run should wait");
        let waiting = store.get_run(run.id).await.unwrap().unwrap();
        assert_eq!(waiting.status, RunStatus::Waiting);
        assert_eq!(waiting.wait_token.as_deref(), Some("token-1"));
        // "Restart": a brand-new executor over the same store.
        let mut workflows2 = HashMap::new();
        workflows2.insert(
            "approval-demo".to_string(),
            workflow_fn(|ctx: Ctx<SqliteStore>, _: serde_json::Value| {
                Box::pin(async move {
                    let _v: String = ctx
                        .step("prepare", || async { Ok("prepared".to_string()) })
                        .await?;
                    let approval = ctx.wait_for_approval("approval", "token-1").await?;
                    let done: String = ctx
                        .step("finish", || async { Ok("finished".to_string()) })
                        .await?;
                    Ok(serde_json::json!({"approval": approval, "done": done}))
                })
            }),
        );
        let executor2 = Executor::new(store.clone(), workflows2, RetryPolicy::default());
        store
            .record_approval(
                "token-1",
                serde_json::json!({"by": "human"}),
                Utc::now().timestamp_millis(),
            )
            .await
            .unwrap();
        let done = executor2
            .run_until_terminal(run.id, Duration::from_secs(5))
            .await
            .expect("run should complete");
        assert_eq!(done.status, RunStatus::Completed);
        assert_eq!(done.output.unwrap()["done"], "finished");
        // "prepare" executed once: on resume it came from the journal.
        assert_eq!(store.list_steps(run.id).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn timer_wait_wakes_and_completes() {
        let store = SqliteStore::connect_memory().await.unwrap();
        store.migrate().await.unwrap();
        let workflow: WorkflowFn<SqliteStore> = Arc::new(|ctx, _| {
            Box::pin(async move {
                ctx.sleep("delay", Duration::from_millis(300)).await?;
                let done: String = ctx
                    .step("notify", || async { Ok("notified".to_string()) })
                    .await?;
                Ok(serde_json::json!({"done": done}))
            })
        });
        let mut workflows = HashMap::new();
        workflows.insert("timer-demo".to_string(), workflow);
        let executor = Executor::new(store.clone(), workflows, RetryPolicy::default());
        let run = store
            .create_run(NewRun::new("timer-demo", serde_json::json!({}), 3))
            .await
            .unwrap();
        // The executor keeps polling; the timer becomes due and the
        // run completes within the deadline.
        let done = executor
            .run_until_terminal(run.id, Duration::from_secs(5))
            .await
            .expect("run should complete");
        assert_eq!(done.status, RunStatus::Completed);
    }
}
