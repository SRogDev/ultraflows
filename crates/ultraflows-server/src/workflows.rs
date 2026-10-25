//! Demo workflows registered with the v0.1.0 server.
//!
//! These are deliberately business-flavored but domain-free: they show
//! the runtime's primitives (journaled steps, retries, timer waits,
//! human-approval waits, idempotency keys) without coupling UltraFlows
//! to any product's logic. External calls are simulated — the point of
//! the demo is durability, not the fake services.

use std::collections::HashMap;
use ultraflows::{Ctx, WorkflowFn, workflow_fn};
use ultraflows_store::SqliteStore;

pub type Store = SqliteStore;

/// `client-onboarding`: validate → create account → wait for a human
/// approval → provision → welcome. The approval wait is the crash
/// proof: kill the server while it waits, restart it, and the run
/// resumes with its earlier steps replayed from the journal, not
/// re-executed.
fn client_onboarding() -> WorkflowFn<Store> {
    workflow_fn(|ctx: Ctx<Store>, payload: serde_json::Value| {
        Box::pin(async move {
            let company = payload["company"].as_str().unwrap_or("unknown").to_string();
            let validated: String = ctx
                .step("validate", move || async { Ok("valid".to_string()) })
                .await?;
            let _ = validated;
            let idem = ctx.idempotency_key("create-account");
            let account: String = ctx
                .step("create-account", move || async move {
                    // Simulated external system: it receives `idem`, so
                    // even if this step were re-executed, the external
                    // effect would be neutralized by the key.
                    Ok(format!("acct-{idem}"))
                })
                .await?;
            let token = format!("onboarding:{company}");
            let approval = ctx.wait_for_approval("approval", &token).await?;
            let provisioned: String = ctx
                .step("provision", || async { Ok("provisioned".to_string()) })
                .await?;
            let welcomed: String = ctx
                .step("welcome", || async { Ok("welcomed".to_string()) })
                .await?;
            Ok(serde_json::json!({
                "account": account,
                "approval": approval,
                "provisioned": provisioned,
                "welcomed": welcomed,
            }))
        })
    })
}

/// `flaky-report`: a step that fails on attempts 1..=N (payload
/// `fail_times`) and succeeds after. Shows retry backoff and that
/// failures record nothing, so the step genuinely re-runs — while a
/// recorded step never does.
fn flaky_report() -> WorkflowFn<Store> {
    workflow_fn(|ctx: Ctx<Store>, payload: serde_json::Value| {
        Box::pin(async move {
            let fail_times = payload["fail_times"].as_u64().unwrap_or(2) as u32;
            let attempt = ctx.attempt();
            let report: String = ctx
                .step("generate", move || async move {
                    if attempt <= fail_times {
                        Err("simulated outage".to_string())
                    } else {
                        Ok("report-ready".to_string())
                    }
                })
                .await?;
            Ok(serde_json::json!({"report": report, "attempts_used": attempt}))
        })
    })
}

/// `delayed-reminder`: wait `seconds` (a persisted timer, no process
/// held) and then notify. Shows timer waits surviving a restart.
fn delayed_reminder() -> WorkflowFn<Store> {
    workflow_fn(|ctx: Ctx<Store>, payload: serde_json::Value| {
        Box::pin(async move {
            let seconds = payload["seconds"].as_u64().unwrap_or(20);
            ctx.sleep("reminder-delay", std::time::Duration::from_secs(seconds))
                .await?;
            let notified: String = ctx
                .step("notify", || async { Ok("notified".to_string()) })
                .await?;
            Ok(serde_json::json!({"notified": notified}))
        })
    })
}

pub fn registry() -> HashMap<String, WorkflowFn<Store>> {
    let mut map = HashMap::new();
    map.insert("client-onboarding".to_string(), client_onboarding());
    map.insert("flaky-report".to_string(), flaky_report());
    map.insert("delayed-reminder".to_string(), delayed_reminder());
    map
}

/// Workflows whose names the API accepts.
pub fn names() -> Vec<&'static str> {
    vec!["client-onboarding", "flaky-report", "delayed-reminder"]
}
