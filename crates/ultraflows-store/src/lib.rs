//! `ultraflows-store` — persistence for UltraFlows.
//!
//! Decision D3 (ADR-0003, option C): storage sits behind a `Store` trait
//! with several backends from day one:
//!
//! - **PostgreSQL** — the real backend for the service (decision D2).
//! - **SQLite** — tests and zero-infrastructure local development.
//! - **In-memory** — fast unit tests of the runtime's logic.
//!
//! The trait is the contract; which backend is "real" must be invisible
//! to the runtime's correctness rules. Dialect differences that leak
//! through the trait are bugs to record in the ADR, not to normalize
//! silently.
//!
//! Status: scaffold only (Phase 0). No storage behavior is implemented yet.
