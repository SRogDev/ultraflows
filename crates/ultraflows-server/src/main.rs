//! UltraFlows server — scaffold entry point (Phase 0).
//!
//! Decision D2 (ADR-0002): UltraFlows is **service-first** — one binary
//! hosting the API, the scheduler and the worker gateway, built on top of
//! the `ultraflows` runtime library and the pure `ultraflows-core`.
//!
//! No server behavior is implemented yet: this entry point only proves the
//! binary builds, initializes tracing, and exits honestly.

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "ultraflows-server scaffold (Phase 0): no runtime implemented yet"
    );
}
