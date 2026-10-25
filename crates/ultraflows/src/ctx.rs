use crate::error::FlowError;
use chrono::{Duration, Utc};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::future::Future;
use ultraflows_core::types::{RunId, StepRecord, WaitRecord, idempotency_key};
use ultraflows_store::Store;

/// The context a workflow function receives. This is the *only* way
/// workflow code touches side effects, time or randomness in v0.1.0:
///
/// - `step` — run an effect, or return its recorded result (replay).
/// - `sleep` — a durable timer wait (persisted, no process held).
/// - `wait_for_approval` — a human-approval waitpoint (persisted).
/// - `idempotency_key` — the deterministic key to forward to external
///   systems so a re-executed step cannot double-apply its effect.
#[derive(Clone)]
pub struct Ctx<S: Store> {
    store: S,
    run_id: RunId,
    attempt: u32,
}

impl<S: Store> Ctx<S> {
    pub fn new(store: S, run_id: RunId, attempt: u32) -> Self {
        Self {
            store,
            run_id,
            attempt,
        }
    }

    pub fn run_id(&self) -> RunId {
        self.run_id
    }

    /// 1-based attempt number of the current execution try. Deterministic
    /// given the run's history — safe to branch on (used by the demo's
    /// flaky workflow: fail on attempts 1..=N, succeed after).
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn idempotency_key(&self, step_key: &str) -> String {
        idempotency_key(self.run_id, step_key)
    }

    async fn check_cancelled(&self) -> Result<(), FlowError> {
        let run = self.store.get_run(self.run_id).await?;
        if run.map(|r| r.cancel_requested).unwrap_or(false) {
            return Err(FlowError::Cancelled);
        }
        Ok(())
    }

    /// Execute `f` once, or return the recorded result on replay.
    ///
    /// The closure is *not* executed when a result for `key` is already
    /// recorded — that is the whole durability trick. A closure that
    /// fails records nothing, so the next attempt re-executes it.
    pub async fn step<T, F, Fut>(&self, key: &str, f: F) -> Result<T, FlowError>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, String>>,
    {
        self.check_cancelled().await?;
        if let Some(record) = self.store.get_step(self.run_id, key).await? {
            let value: T = serde_json::from_value(record.result).map_err(|e| {
                FlowError::Failed(format!(
                    "recorded step '{key}' is not a {t}: {e}",
                    t = std::any::type_name::<T>()
                ))
            })?;
            return Ok(value);
        }
        let value = f().await.map_err(FlowError::Failed)?;
        let record = StepRecord {
            run_id: self.run_id,
            key: key.to_string(),
            result: serde_json::to_value(&value).map_err(|e| FlowError::Failed(e.to_string()))?,
            idempotency_key: self.idempotency_key(key),
        };
        if self.store.record_step(&record).await? {
            self.store
                .append_event(self.run_id, "StepRecorded", serde_json::json!({"key": key}))
                .await?;
            Ok(value)
        } else {
            // Lost a race (or a duplicate delivery): the first result
            // wins, so return it instead of our own.
            let record = self.store.get_step(self.run_id, key).await?;
            match record {
                Some(r) => {
                    serde_json::from_value(r.result).map_err(|e| FlowError::Failed(e.to_string()))
                }
                None => Err(FlowError::Failed(format!(
                    "step '{key}' vanished after recording"
                ))),
            }
        }
    }

    /// Durable timer. Returns `Suspended` until the wake time, then Ok.
    /// The executor treats Suspended as "mark Waiting, no retry consumed".
    pub async fn sleep(&self, key: &str, duration: std::time::Duration) -> Result<(), FlowError> {
        self.check_cancelled().await?;
        let now = Utc::now();
        match self.store.get_wait(self.run_id, key).await? {
            Some(wait) => {
                let due = wait.wake_at.map(|w| w <= now).unwrap_or(true);
                if due {
                    self.store.resolve_wait(self.run_id, key).await?;
                    self.clear_run_wait().await?;
                    Ok(())
                } else {
                    Err(FlowError::Suspended)
                }
            }
            None => {
                let wake_at = now + Duration::from_std(duration).unwrap_or(Duration::seconds(0));
                self.store
                    .upsert_wait(&WaitRecord {
                        run_id: self.run_id,
                        key: key.to_string(),
                        kind: "timer".to_string(),
                        token: None,
                        wake_at: Some(wake_at),
                    })
                    .await?;
                self.set_run_wait(key, "timer", None, Some(wake_at)).await?;
                self.store
                    .append_event(
                        self.run_id,
                        "WaitCreated",
                        serde_json::json!({"key": key, "kind": "timer"}),
                    )
                    .await?;
                Err(FlowError::Suspended)
            }
        }
    }

    /// Human-approval waitpoint. Returns the approval result once the
    /// token is completed via the API, `Suspended` until then.
    pub async fn wait_for_approval(
        &self,
        key: &str,
        token: &str,
    ) -> Result<serde_json::Value, FlowError> {
        self.check_cancelled().await?;
        if let Some(result) = self.store.get_approval(token).await? {
            self.store.resolve_wait(self.run_id, key).await?;
            self.clear_run_wait().await?;
            return Ok(result);
        }
        if self.store.get_wait(self.run_id, key).await?.is_none() {
            self.store
                .upsert_wait(&WaitRecord {
                    run_id: self.run_id,
                    key: key.to_string(),
                    kind: "approval".to_string(),
                    token: Some(token.to_string()),
                    wake_at: None,
                })
                .await?;
            self.set_run_wait(key, "approval", Some(token.to_string()), None)
                .await?;
            self.store
                .append_event(
                    self.run_id,
                    "WaitCreated",
                    serde_json::json!({"key": key, "kind": "approval", "token": token}),
                )
                .await?;
        }
        Err(FlowError::Suspended)
    }

    async fn set_run_wait(
        &self,
        key: &str,
        kind: &str,
        token: Option<String>,
        wake_at: Option<chrono::DateTime<Utc>>,
    ) -> Result<(), FlowError> {
        if let Some(mut run) = self.store.get_run(self.run_id).await? {
            run.wait_key = Some(key.to_string());
            run.wait_kind = Some(kind.to_string());
            run.wait_token = token;
            run.wake_at = wake_at;
            run.updated_at = Utc::now();
            self.store.update_run(&run).await?;
        }
        Ok(())
    }

    async fn clear_run_wait(&self) -> Result<(), FlowError> {
        if let Some(mut run) = self.store.get_run(self.run_id).await? {
            run.wait_key = None;
            run.wait_kind = None;
            run.wait_token = None;
            run.wake_at = None;
            run.updated_at = Utc::now();
            self.store.update_run(&run).await?;
        }
        Ok(())
    }
}
