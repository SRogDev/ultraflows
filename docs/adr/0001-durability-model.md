# ADR-0001 — Durability model: journaled replay, Temporal-style

- Status: **Accepted** (Roger, 2026-10-23 — "sí, como Temporal")
- Deciders: Roger (choice), Muse (framing)

## Context

A workflow must survive process crashes and restarts over months. Three
models were compared: (A) journaled step memoization + deterministic
replay, (B) an interpreted state machine defined as data, (C) persisted
continuations / process checkpoints (CRIU-style).

## Decision

Adopt **A, modeled on Temporal's semantics**: an append-only event
history is the source of truth for a run; workflow code is deterministic
and re-executes from the top on resume; every side effect lives in a
named step/activity whose recorded result is replayed instead of
re-executed.

## Consequences

- Code between steps must be deterministic (no wall-clock, randomness,
  direct I/O or unordered iteration feeding decisions) — the SDK must
  provide durable replacements (`now`, timers, random/UUID generation
  recorded in history), exactly as Temporal does.
- Non-determinism (including all AI calls) lives inside steps, never in
  workflow code.
- Replay/versioning of long-lived runs becomes a first-class problem
  (workflow versioning / patching) — Phase 2 records it; Phase 1 must not
  make it impossible.

## Revisit if

A workload appears whose logic cannot be expressed deterministically
without contortions that a data-defined state machine (B) handles
clearly better — then B may be added *for that class*, not as a rewrite.
