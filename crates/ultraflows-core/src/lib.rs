//! `ultraflows-core` — the pure heart of UltraFlows.
//!
//! Workflow/run/step/attempt types, the run state machine, the retry
//! policy, and the correctness invariants of the runtime.
//!
//! It deliberately depends on **no async runtime, no storage driver and no
//! I/O**: every rule in here is testable synchronously. If a type needs
//! Tokio or a database to be tested, it belongs in another crate.

pub mod error;
pub mod retry;
pub mod schedule;
pub mod state;
pub mod types;

pub use error::CoreError;
pub use retry::RetryPolicy;
pub use schedule::next_fire;
pub use state::{can_transition, check_transition};
pub use types::{NewRun, Run, RunEvent, RunId, RunStatus, StepRecord, WaitRecord, idempotency_key};
