# ROADMAP — UltraFlows

Four progressive phases. A phase is done when its exit criteria are met
with evidence (tests, fault-injection runs, benchmark reports) — not when
the code merely exists. The system must stay working at every phase.

## Phase 0 — Repository and development workflow (current)

- Workspace layout, pinned toolchain, deliberate dependency set.
- README / ROADMAP / ARCHITECTURE / TESTING / SECURITY_RELIABILITY.
- ADRs for the decisions known so far (provisional where Roger has not
  yet decided — see `docs/adr/`).
- Exit: `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt --check`
  all green on the scaffold; zero runtime behavior claimed.

## Phase 1 — Minimal durable core (service form, single node)

**v0.1.0 DONE (2026-10-25, portfolio scope — see ADR-0008):** the core
slice + service shell over SQLite with an in-process worker, 11 tests
and the kill-restart demo. Remaining Phase 1 work: lease expiry +
Running-run recovery, PostgreSQL backend, remote worker protocol.

Decisions D1–D4 (ADRs 0001–0004): Temporal-style replay, service-first,
Store trait (Postgres real), code-first Rust definitions. Built in two
steps so the system works at each: **1A** core slice (types, journal,
replay, retries against the in-memory/SQLite store) and **1B** service
shell (server binary, Postgres backend, minimal worker protocol, Rust
worker running the example end-to-end).

Scope: define a workflow in Rust, run it through the service, journal
its steps, survive a process kill, resume without re-executing
recorded steps.

Exit criteria:

1. An example workflow of ≥5 steps reaches `Completed`; its full history
   is in an append-only event log.
2. Fault injection (kill at random points, ≥20 runs): after restart the
   run completes and no *recorded* step re-executes (journal audit).
3. A step failing N−1 times is retried per its declared policy (backoff
   within tolerance); exhausting `maxAttempts` leaves the run `Failed`
   with the last error preserved.
4. `wait.for(60s)` survives a restart during the wait and never wakes early.
5. Cancellation: immediate for waiting runs; at the next step boundary
   for running runs — documented semantics, tested.
6. Every step gets a deterministic idempotency key derived from
   `(run_id, step_key)`; the example proves a duplicated external effect
   is neutralized by it.
7. Invariants (see ARCHITECTURE.md) have unit + property tests.
8. All Phase 0 checks stay green; README states what Phase 1 does NOT do.

## Phase 2 — Robustness

Timeouts per step/run, concurrency limits (total + per key), schedules
(cron, IANA timezones, DST), human-approval waitpoints with timeouts,
systematic property tests and fault injection, observability (tracing
spans per run/step/attempt).

## Phase 3 — Service hardening + Polygrow integration seam

(The service itself is Phase 1 per ADR-0002; this phase hardens it.)
Per-environment auth with hashed keys, multi-tenancy and tenant
isolation, worker versioning and definition-drift handling, scheduler
leader election, payload-size limits/externalization, and the Polygrow
integration layer — which lives *outside* this runtime (business
context, authorization, agent orchestration, tool contracts).

## Phase 4 — Measured optimization and scale

Benchmarks against a fixed, published hardware/workload manifest
(timer-wheel efficiency, dispatch latency, journal throughput, cost of
100k waiting runs), backpressure and resource limits, large-payload
externalization, multi-node dispatch if — and only if — measurements
justify it. No optimization without a measured bottleneck.

## Open questions

Tracked in `docs/adr/`. ADR-0001…0005 and 0007 are **Accepted**
(Roger's decisions, 2026-10-23). Still open: name + license
(ADR-0006). Anything else unspecified defaults to Temporal's
documented semantics (ADR-0007), recorded where used.

## Trigger-core increment (DONE 2026-10-25, ADR-0009)

Remote-worker runtime replacing the v0.1.0 in-process demo: task
registry, Trigger-shaped API, lease protocol + Rust SDK, external
demo app, cron schedules (DST-tested), idempotent triggers, TTL /
delay / reschedule, concurrency caps at claim time. 25 tests +
18-proof demo script green. 2026-10-25 follow-up (ADR-0010):
Postgres backend DONE (live PG 16.15, 15-proof demo) and Python SDK
DONE (FastAPI + LangGraph example). Remaining Phase 1 work:
declarative schedules, batch waitpoints, webhook waiters (in that
order of runtime value).
