//! UltraFlows server — service-first (ADR-0002).
//!
//! One binary: HTTP API + scheduler + worker loop, over the `ultraflows`
//! runtime and a SQLite store. Run it with:
//!
//! ```sh
//! ULTRAFLOW_DB=ultraflows.db PORT=3000 cargo run --bin ultraflows-server
//! ```
//!
//! Then see `scripts/demo.sh` for a full end-to-end demonstration,
//! including a `kill -9` restart while a run waits on a human approval.

mod api;
mod workflows;

use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use ultraflows::Executor;
use ultraflows_core::RetryPolicy;
use ultraflows_store::{SqliteStore, Store};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let db_path = std::env::var("ULTRAFLOW_DB").unwrap_or_else(|_| "ultraflows.db".to_string());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    // sqlx SQLite URLs: "sqlite:ultraflows.db" creates the file.
    let url = if db_path.starts_with("sqlite:") {
        db_path
    } else {
        format!("sqlite:{db_path}?mode=rwc")
    };
    let store = SqliteStore::connect(&url).await.expect("open database");
    store.migrate().await.expect("migrate database");

    let executor = Executor::new(store, workflows::registry(), RetryPolicy::default());
    let state = Arc::new(api::AppState {
        executor: executor.clone(),
    });

    // The worker loop: single worker in this binary (v0.1.0). Remote
    // workers over a worker protocol are a later phase.
    let worker = executor.clone();
    tokio::spawn(async move {
        worker.drive(Duration::from_millis(100)).await;
    });

    let app = api::router(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind address");
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        port,
        workflows = ?workflows::names(),
        "ultraflows-server listening on 127.0.0.1 (local only, no auth in v0.1.0)"
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = signal::ctrl_c().await;
            tracing::info!("shutting down");
        })
        .await
        .expect("serve");
}
