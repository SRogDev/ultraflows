# STATUS — UltraFlows

## Done

- 2026-10-23 — Phase 0 scaffold: workspace (edition 2024, Rust 1.98.1),
  4 crates (core / store / runtime / server), deliberate dependency set
  downloaded and locked, base docs (README, ROADMAP, ARCHITECTURE,
  TESTING, SECURITY_RELIABILITY), ADRs 0001–0007.
- 2026-10-23 — Foundational decisions (Roger): D1 durability =
  Temporal-style journaled replay; D2 = service-first; D3 = Store trait
  (Postgres real, SQLite/memory for tests); D4 = code-first Rust
  definitions; unspecified behavior defaults to Temporal (ADR-0007).
- 2026-10-23 — Phase 0 verified: `cargo build`, `cargo test` (0 tests —
  scaffold, honestly), `cargo clippy -D warnings` and `cargo fmt
  --check` all green; `ultraflows-server` binary runs and logs its
  scaffold status. Stack resolved in Cargo.lock: tokio 1.54.1,
  sqlx 0.8.6, serde 1.0.229, chrono 0.4.45, uuid 1.26.1, proptest 1.11.0.

## Doing

- Phase 1A design sessions (mentored): core types, event history,
  step journal contract, Store trait contract.

## Next

- Phase 1A implementation (only on Roger's go): minimal durable slice
  against in-memory/SQLite store, fault-injection harness.
- Phase 1B: server + Postgres + minimal worker protocol.
- Open: final name and license (ADR-0006).
