//! UltraFlows — a native Rust durable workflow runtime.
//!
//! UltraFlows executes long-running workflows — steps, dependencies, waits,
//! retries, schedules and human approvals — in a way that survives process
//! crashes and restarts. It is being built as the future durable-execution
//! layer for Polygrow, but knows nothing about Polygrow.
//!
//! # The model (Temporal-shaped, ADR-0001/0004)
//!
//! - Workflow functions are **deterministic orchestration**: they call
//!   [`Ctx::step`], [`Ctx::sleep`] and [`Ctx::wait_for_approval`].
//! - Every side effect lives inside a step. A recorded step never
//!   re-executes for that run — on retry/resume the workflow function
//!   re-runs from the top and steps return their journaled results.
//! - Waits are persisted data, not sleeping processes: a waiting run
//!   holds no worker slot.
//!
//! # Honesty contract
//!
//! - Workflow attempts execute **at-least-once**.
//! - A step whose result was recorded is never executed again for that run.
//! - An external side effect is exactly-once **only** if the external
//!   system accepts the idempotency key from [`Ctx::idempotency_key`].
//!   There is no universal exactly-once, and this crate never claims one.
//!
//! # v0.1.0 scope
//!
//! Single worker, SQLite backend, in-process workflows. See README.md
//! for the full list of what is and is not implemented.

pub mod ctx;
pub mod error;
pub mod executor;

pub use ctx::Ctx;
pub use error::FlowError;
pub use executor::{Executor, WorkflowFn, workflow_fn};
