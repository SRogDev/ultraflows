# ADR-0005 — Phase 1 stack

- Status: **Accepted** (scaffold, 2026-10-23)

## Decision

- **Rust edition 2024**, toolchain pinned (1.98.1) in `rust-toolchain.toml`.
- **Tokio** — async runtime for the server, scheduler and workers only.
  `ultraflows-core` must never depend on it: the state machine stays
  synchronously testable.
- **serde / serde_json** — payloads and step results.
- **thiserror** — typed errors in library crates (no `anyhow` there).
- **tracing / tracing-subscriber** — structured observability from day 1.
- **uuid (v7)** — time-ordered run/step/attempt IDs.
- **chrono** — persisted timestamps; IANA/DST cron semantics get their
  own ADR in Phase 2.
- **sqlx** — storage driver for both real and test backends (ADR-0003).
- **proptest** (dev) — property tests over the invariants.
- **axum** — the server's HTTP layer. *Not yet a dependency*: it enters
  with the first real endpoint (Phase 1B), not the scaffold.

## Explicitly NOT adopted (yet)

- `criterion` — added when the first real benchmark is written.
- Any queue/broker (Redis, NATS, Kafka) — PostgreSQL is queue, journal
  and lock manager in v1, per the Trigger-recon spec's reasoning.
- Any ORM — sqlx with explicit SQL; the journal's queries are the product.
