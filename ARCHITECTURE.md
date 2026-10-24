# ARCHITECTURE — UltraFlows

## Boundaries (non-negotiable)

1. **Workflow runtime** (this repo): durable execution state, scheduling,
   transitions, waits, retries, recovery, cancellation, concurrency policy.
2. **Polygrow integration layer** (another repo/layer): business context,
   authorization, agent orchestration, approvals UX, capability selection,
   tool invocation contracts.
3. **Execution environments**: isolated sandboxes that run generated or
   otherwise unsafe code. The runtime schedules work; it does not itself
   become a sandbox.
4. **AI decision-making**: planning and semantic evaluation live outside.
   To the runtime, an AI call is an external step with a recorded result.

## Crate map

- `ultraflows-core` — pure types and rules: definitions, runs, steps,
  attempts, the run state machine, retry policies, invariants. No Tokio,
  no I/O, no storage. Testable synchronously and with property tests.
- `ultraflows-store` — the `Store` trait + backends (ADR-0003):
  PostgreSQL is the real backend; SQLite and in-memory serve tests and
  local development, all under one conformance suite.
- `ultraflows` — the runtime library: the API a developer writes
  against (workflows, activities/steps, waits, triggers) plus the
  Tokio scheduling/execution machinery.
- `ultraflows-server` — the service binary (ADR-0002, service-first):
  API + scheduler + worker gateway in one process; roles split later
  only if a deployment ever needs it.

## Core invariants (Phase 1 draft — to be finalized with ADR-0001)

- I1. Run state changes only through the state machine's valid transitions.
- I2. `(run_id, step_key)` has at most one recorded result, forever.
- I3. A terminal run (`Completed`, `Failed`, `Cancelled`, `Expired`)
  never executes another step.
- I4. A wait never resolves before its wake time.
- I5. Persisted state is the source of truth; in-memory state (timers,
  counters) is always rebuildable from it after a crash.
- I6. Every state transition appends an event to the run's event log;
  per-run event sequence numbers have no gaps.
- I7. Retries, timeouts, cancellation and concurrency limits are explicit
  configuration — never hidden defaults that surprise.

## Durability model

Decided (ADR-0001, Temporal-style): journaled step memoization +
deterministic replay over an append-only event history. A workflow function re-executes from the top on resume; each
`step()` first consults the journal — a hit returns the recorded result
without executing. Waits are persisted data, not sleeping processes.

The honest window: a step whose effect happened but whose record never
committed *will* re-execute. The mitigation is the propagated
idempotency key — see the honesty contract in `README.md`.

## Unresolved

- Exact workflow-definition API (code-first shape) — follows ADR-0001/0004.
- Whether replay determinism is enforced, linted, or only documented.
- Definition-version pinning for long-lived runs (Phase 2+ concern,
  recorded now so Phase 1 does not paint it into a corner).
