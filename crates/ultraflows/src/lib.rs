//! UltraFlows — a native Rust durable workflow runtime (temporary name).
//!
//! UltraFlows executes long-running workflows — steps, dependencies, waits,
//! retries, schedules and human approvals — in a way that survives process
//! crashes and restarts, without coupling to any business domain or agent
//! framework. It is being built as the future durable-execution layer for
//! Polygrow, but knows nothing about Polygrow.
//!
//! # Honesty contract
//!
//! - Workflow attempts execute **at-least-once**.
//! - A step whose result was recorded is never executed again for that run.
//! - An external side effect is exactly-once **only** if the external
//!   system accepts the idempotency key UltraFlows propagates. There is no
//!   universal exactly-once, and this crate will never claim one.
//!
//! Status: scaffold only (Phase 0). No runtime behavior is implemented yet.
//! See `README.md`, `ROADMAP.md` and `ARCHITECTURE.md` in the repository
//! root, and the ADRs in `docs/adr/`.
