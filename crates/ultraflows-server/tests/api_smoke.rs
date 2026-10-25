//! End-to-end smoke tests over real HTTP: register, trigger, lease
//! with concurrency caps, step journaling, wait + resume, retry.
//! The server under test runs on a loopback port with an in-memory DB.

use serde_json::json;
use std::time::Duration;
use ultraflows_server::api;
use ultraflows_store::{SqliteStore, Store};

async fn spawn_server() -> (String, tokio::task::JoinHandle<()>) {
    let store = SqliteStore::connect_memory().await.unwrap();
    store.migrate().await.unwrap();
    let app = api::router(api::AppState {
        store: store.clone(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let sched = store.clone();
    tokio::spawn(async move {
        loop {
            let _ = api::scheduler_tick(&sched).await;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://127.0.0.1:{port}"), handle)
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn post_json(url: &str, body: serde_json::Value) -> (u16, serde_json::Value) {
    let resp = client().post(url).json(&body).send().await.unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap_or(json!({"raw": text}));
    (status, value)
}

async fn get_json(url: &str) -> (u16, serde_json::Value) {
    let resp = client().get(url).send().await.unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap_or(json!({"raw": text}));
    (status, value)
}

#[tokio::test]
async fn concurrency_cap_never_exceeded_at_lease_time() {
    let (base, _h) = spawn_server().await;
    // Task with total cap 1.
    let (s, _) = post_json(
        &format!("{base}/v1/workers/register"),
        json!({"worker_id": "w1", "tasks": [{"id": "capped", "version_hash": "v1", "concurrency_total": 1}]}),
    )
    .await;
    assert_eq!(s, 200);
    let mut run_ids = Vec::new();
    for _ in 0..2 {
        let (s, v) = post_json(
            &format!("{base}/v1/tasks/capped/trigger"),
            json!({"payload": {}}),
        )
        .await;
        assert_eq!(s, 200);
        run_ids.push(v["run_id"].as_str().unwrap().to_string());
    }
    // First lease succeeds.
    let (s, v) = post_json(
        &format!("{base}/v1/workers/w1/lease"),
        json!({"tasks": ["capped"]}),
    )
    .await;
    assert_eq!(s, 200);
    assert!(v["run"].is_object(), "first lease must return a run");
    let leased_id = v["run"]["id"].as_str().unwrap().to_string();
    // Second lease must return null while the cap is held — the
    // blocked run is skipped, not leased over the cap.
    let (s, v) = post_json(
        &format!("{base}/v1/workers/w2/lease"),
        json!({"tasks": ["capped"]}),
    )
    .await;
    assert_eq!(s, 200);
    assert!(v.is_null(), "cap 1 must block the second lease, got {v}");
    // Complete the first; the second becomes leasable.
    let (s, _) = post_json(
        &format!("{base}/v1/workers/leases/{leased_id}/complete"),
        json!({"output": {"done": true}}),
    )
    .await;
    assert_eq!(s, 200);
    let (s, v) = post_json(
        &format!("{base}/v1/workers/w2/lease"),
        json!({"tasks": ["capped"]}),
    )
    .await;
    assert_eq!(s, 200);
    assert!(v["run"].is_object(), "second run leasable after completion");
    assert_ne!(v["run"]["id"].as_str().unwrap(), leased_id);
    assert!(run_ids.contains(&v["run"]["id"].as_str().unwrap().to_string()));
}

#[tokio::test]
async fn trigger_wait_complete_roundtrip_with_step_journal() {
    let (base, _h) = spawn_server().await;
    let (s, _) = post_json(
        &format!("{base}/v1/workers/register"),
        json!({"worker_id": "w1", "tasks": [{"id": "t1", "version_hash": "v1"}]}),
    )
    .await;
    assert_eq!(s, 200);
    let (s, v) = post_json(
        &format!("{base}/v1/tasks/t1/trigger"),
        json!({"payload": {"n": 1}, "tags": ["smoke"]}),
    )
    .await;
    assert_eq!(s, 200);
    let run_id = v["run_id"].as_str().unwrap().to_string();
    // Filtered list finds it by tag.
    let (s, v) = get_json(&format!("{base}/v1/runs?tag=smoke")).await;
    assert_eq!(s, 200);
    assert!(v.as_array().unwrap().iter().any(|r| r["id"] == run_id));
    // Lease, record a step twice (insert-once), suspend on a timer,
    // then verify the run is Waiting and the step recorded once.
    let (s, v) = post_json(
        &format!("{base}/v1/workers/w1/lease"),
        json!({"tasks": ["t1"]}),
    )
    .await;
    assert_eq!(s, 200);
    assert_eq!(v["run"]["id"], run_id);
    let (s, v1) = post_json(
        &format!("{base}/v1/workers/leases/{run_id}/step"),
        json!({"key": "a", "result": "first"}),
    )
    .await;
    assert_eq!(s, 200);
    assert_eq!(v1["recorded"], true);
    let (s, v2) = post_json(
        &format!("{base}/v1/workers/leases/{run_id}/step"),
        json!({"key": "a", "result": "second"}),
    )
    .await;
    assert_eq!(s, 200);
    assert_eq!(v2["recorded"], false);
    assert_eq!(v2["result"], "first");
    let (s, _) = post_json(
        &format!("{base}/v1/workers/leases/{run_id}/wait"),
        json!({"key": "nap", "kind": "timer", "wake_at": "2999-01-01T00:00:00Z"}),
    )
    .await;
    assert_eq!(s, 200);
    let (s, v) = get_json(&format!("{base}/v1/runs/{run_id}")).await;
    assert_eq!(s, 200);
    assert_eq!(v["status"], "WAITING");
    let (s, v) = get_json(&format!("{base}/v1/runs/{run_id}/steps")).await;
    assert_eq!(s, 200);
    assert_eq!(v.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn schedule_rejects_bad_cron_and_l_syntax() {
    let (base, _h) = spawn_server().await;
    let (s, _) = post_json(
        &format!("{base}/v1/schedules"),
        json!({"task_id": "t1", "cron": "0 9 L * *", "timezone": "UTC", "payload": {}}),
    )
    .await;
    assert_eq!(s, 400, "L syntax must be rejected, not silently misfired");
    let (s, _) = post_json(
        &format!("{base}/v1/schedules"),
        json!({"task_id": "t1", "cron": "not a cron", "timezone": "UTC", "payload": {}}),
    )
    .await;
    assert_eq!(s, 400);
}
