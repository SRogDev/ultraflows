//! UltraFlows server — service-first (ADR-0002).
//!
//! One binary: HTTP API + scheduler loop. The server schedules work
//! and serves the worker protocol; it never runs task code itself —
//! remote workers (see the `ultraflows-sdk` crate and
//! `ultraflows-demo-app`) lease runs and execute them. Run it with:
//!
//! ```sh
//! ULTRAFLOW_DB=ultraflows.db PORT=3000 cargo run --bin ultraflows-server
//! ```
//!
//! Then see `scripts/trigger-demo.sh` for a full end-to-end
//! demonstration, including a `kill -9` restart while a run waits on
//! a human approval.

mod api;

use std::time::Duration;
use tokio::signal;
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

    // Scheduler loop: TTL expiry + cron schedule firing, every second.
    // Timer waits need no waking here: a Waiting run whose wake time
    // passed is directly leaseable by workers (see claim_specific).
    let sched_store = store.clone();
    tokio::spawn(async move {
        loop {
            match api::scheduler_tick(&sched_store).await {
                Ok(fired) => {
                    for id in fired {
                        tracing::info!(run_id = %id, "schedule fired");
                    }
                }
                Err(e) => tracing::error!("scheduler tick error: {e}"),
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    let app = api::router(api::AppState { store });
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind address");
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        port,
        "ultraflows-server listening on 127.0.0.1 (local only, no auth)"
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = signal::ctrl_c().await;
            tracing::info!("shutting down");
        })
        .await
        .expect("serve");
}
