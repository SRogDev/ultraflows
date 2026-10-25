//! External demo app — a separate binary that proves other apps can
//! send jobs to UltraFlows. It is NOT part of the server.
//!
//! Usage (server must already be running):
//! - `ultraflows-demo-app worker` — register the demo tasks and work
//!   leased runs until killed.
//! - `ultraflows-demo-app trigger-all` — trigger one run of each demo
//!   task plus a schedule and an idempotent re-trigger; prints JSON
//!   with the run ids for `scripts/trigger-demo.sh` to check.
//!
//! Server URL: `ULTRAFLOWS_URL` (default `http://127.0.0.1:3000`).

use std::sync::Arc;
use std::time::Duration;
use ultraflows_sdk::{Client, RemoteCtx, SdkError, TaskDef, TaskFn, TriggerOptions, Worker};

fn task_fn<F, Fut>(f: F) -> TaskFn
where
    F: Fn(RemoteCtx, serde_json::Value) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<serde_json::Value, SdkError>> + Send + 'static,
{
    Arc::new(move |ctx, payload| Box::pin(f(ctx, payload)))
}

fn build_worker(client: Client) -> Worker {
    Worker::new(client, "demo-worker-1")
        .task(
            TaskDef {
                id: "onboarding".to_string(),
                version_hash: "demo-v1".to_string(),
                retry: None,
                concurrency_total: None,
                concurrency_per_key: None,
                queue: None,
            },
            task_fn(|ctx, payload| async move {
                let email: String = ctx
                    .step("welcome-email", || async {
                        Ok(format!(
                            "sent to {}",
                            payload["email"].as_str().unwrap_or("user")
                        ))
                    })
                    .await?;
                let child = ctx
                    .trigger_and_wait(
                        "charge",
                        "charge-card",
                        serde_json::json!({"amount": payload["amount"].as_u64().unwrap_or(0)}),
                    )
                    .await?;
                let done: String = ctx
                    .step("finalize", || async { Ok("onboarded".to_string()) })
                    .await?;
                Ok(serde_json::json!({"email": email, "child": child, "done": done}))
            }),
        )
        .task(
            TaskDef {
                id: "charge-card".to_string(),
                version_hash: "demo-v1".to_string(),
                retry: None,
                concurrency_total: None,
                concurrency_per_key: None,
                queue: None,
            },
            task_fn(|ctx, payload| async move {
                let receipt: String = ctx
                    .step("charge", || async {
                        Ok(format!(
                            "receipt-{}",
                            payload["amount"].as_u64().unwrap_or(0)
                        ))
                    })
                    .await?;
                Ok(serde_json::json!({"receipt": receipt}))
            }),
        )
        .task(
            TaskDef {
                id: "approval-flow".to_string(),
                version_hash: "demo-v1".to_string(),
                retry: None,
                concurrency_total: None,
                concurrency_per_key: None,
                queue: None,
            },
            task_fn(|ctx, payload| async move {
                let prepared: String = ctx
                    .step("prepare", || async { Ok("prepared".to_string()) })
                    .await?;
                let token = payload["token"].as_str().unwrap_or("").to_string();
                let approval = ctx.wait_for_token("approval", &token).await?;
                let done: String = ctx
                    .step("after-approval", || async { Ok("done".to_string()) })
                    .await?;
                Ok(serde_json::json!({"prepared": prepared, "approval": approval, "done": done}))
            }),
        )
        .task(
            TaskDef {
                id: "flaky-report".to_string(),
                version_hash: "demo-v1".to_string(),
                retry: None,
                concurrency_total: None,
                concurrency_per_key: None,
                queue: None,
            },
            task_fn(|ctx, payload| async move {
                let fail_times = payload["fail_times"].as_u64().unwrap_or(0) as u32;
                let out: String = ctx
                    .step("generate", || {
                        let attempt = ctx.attempt();
                        async move {
                            if attempt <= fail_times {
                                Err("simulated outage".to_string())
                            } else {
                                Ok("report-ready".to_string())
                            }
                        }
                    })
                    .await?;
                Ok(serde_json::json!({"report": out}))
            }),
        )
        .task(
            TaskDef {
                id: "timer-task".to_string(),
                version_hash: "demo-v1".to_string(),
                retry: None,
                concurrency_total: None,
                concurrency_per_key: None,
                queue: None,
            },
            task_fn(|ctx, _payload| async move {
                let started: String = ctx
                    .step("start", || async { Ok("started".to_string()) })
                    .await?;
                ctx.sleep("nap", Duration::from_secs(2)).await?;
                let done: String = ctx
                    .step("after-timer", || async { Ok("woke".to_string()) })
                    .await?;
                Ok(serde_json::json!({"started": started, "done": done}))
            }),
        )
        .task(
            TaskDef {
                id: "tick-task".to_string(),
                version_hash: "demo-v1".to_string(),
                retry: None,
                concurrency_total: None,
                concurrency_per_key: None,
                queue: None,
            },
            task_fn(|ctx, _payload| async move {
                let ticked: String = ctx
                    .step("tick", || async { Ok("ticked".to_string()) })
                    .await?;
                Ok(serde_json::json!({"ticked": ticked}))
            }),
        )
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let base =
        std::env::var("ULTRAFLOWS_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".to_string());
    let client = Client::new(base);
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "worker".to_string());
    match mode.as_str() {
        "worker" => {
            let worker = build_worker(client);
            let reg = worker.register().await.expect("register tasks");
            tracing::info!(worker_id = %reg.worker_id, tasks = ?reg.tasks, "registered");
            worker.drive(Duration::from_millis(200)).await;
        }
        "trigger-all" => {
            let approval = client
                .trigger(
                    "approval-flow",
                    TriggerOptions {
                        payload: Some(serde_json::json!({"token": "demo-approval-token"})),
                        ..Default::default()
                    },
                )
                .await
                .expect("trigger approval-flow");
            let flaky = client
                .trigger(
                    "flaky-report",
                    TriggerOptions {
                        payload: Some(serde_json::json!({"fail_times": 2})),
                        ..Default::default()
                    },
                )
                .await
                .expect("trigger flaky-report");
            let timer = client
                .trigger(
                    "timer-task",
                    TriggerOptions {
                        payload: Some(serde_json::json!({})),
                        ..Default::default()
                    },
                )
                .await
                .expect("trigger timer-task");
            let parent = client
                .trigger(
                    "onboarding",
                    TriggerOptions {
                        payload: Some(
                            serde_json::json!({"email": "demo@example.com", "amount": 42}),
                        ),
                        ..Default::default()
                    },
                )
                .await
                .expect("trigger onboarding");
            // Idempotent re-trigger: same key must return the same run.
            let idem_opts = TriggerOptions {
                payload: Some(serde_json::json!({})),
                idempotency_key: Some("demo-idem-key".to_string()),
                ..Default::default()
            };
            let idem1 = client
                .trigger("tick-task", idem_opts.clone())
                .await
                .expect("trigger idempotent 1");
            let idem2 = client
                .trigger("tick-task", idem_opts)
                .await
                .expect("trigger idempotent 2");
            // Every-minute schedule for tick-task.
            let schedule = client
                .create_schedule(
                    "tick-task",
                    "* * * * *",
                    "UTC",
                    serde_json::json!({}),
                    Some("demo-every-minute"),
                )
                .await
                .expect("create schedule");
            println!(
                "{}",
                serde_json::json!({
                    "approval_run_id": approval.run_id,
                    "flaky_run_id": flaky.run_id,
                    "timer_run_id": timer.run_id,
                    "parent_run_id": parent.run_id,
                    "idem_run_id_1": idem1.run_id,
                    "idem_run_id_2": idem2.run_id,
                    "schedule": schedule,
                })
            );
        }
        other => {
            eprintln!("unknown mode: {other} (expected worker|trigger-all)");
            std::process::exit(2);
        }
    }
}
