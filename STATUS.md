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

## Doing

- Phase 1A design sessions (mentored): core types, event history,
  step journal contract, Store trait contract.

## Next

- Phase 1A implementation (only on Roger's go): minimal durable slice
  against in-memory/SQLite store, fault-injection harness.
- Phase 1B: server + Postgres + minimal worker protocol.
- Open: final name and license (ADR-0006).
