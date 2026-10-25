//! `ultraflows-store` — persistence for UltraFlows.
//!
//! Decision D3 (ADR-0003, option C): storage sits behind a `Store` trait
//! with several backends. v0.1.0 implements the trait for SQLite (the
//! demo/production backend of this release); PostgreSQL is the next
//! backend to implement against the same trait, not a claim made here.
//!
//! The trait is the contract; which backend is "real" must be invisible
//! to the runtime's correctness rules.

pub mod error;
pub mod sqlite;
pub mod traits;

pub use error::StoreError;
pub use sqlite::SqliteStore;
pub use traits::Store;
