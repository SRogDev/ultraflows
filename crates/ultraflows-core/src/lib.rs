//! `ultraflows-core` — the pure heart of UltraFlows.
//!
//! This crate will hold the workflow/run/step/attempt types, the run state
//! machine, the retry policy, and the correctness invariants of the runtime.
//!
//! It deliberately depends on **no async runtime, no storage driver and no
//! I/O**: every rule in here must be testable synchronously, including with
//! property-based tests. If a type needs Tokio or a database to be tested,
//! it belongs in another crate.
//!
//! Status: scaffold only (Phase 0). No runtime behavior is implemented yet.
//! See `ARCHITECTURE.md` and `docs/adr/` in the repository root.
